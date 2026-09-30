#![no_std]
#![doc = include_str!("../README.md")]
//! Two-input PWM H-bridge control. Both inputs low means coast.
//!
//! PWM timer setup belongs to the caller. The commutation wait must cover actual
//! peripheral update latency plus the bridge's required dead time. A zero-duty
//! write may take effect only at a timer boundary; it does not stop wheel rotation.
//! Initialization and transitions require exclusive access to both outputs.

use embedded_hal::pwm::SetDutyCycle;
use embedded_hal_async::delay::DelayNs;

/// Caller-assigned nonzero identity; independent of wheel position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotorId(u8);

/// A zero motor identity is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidMotorId;

impl MotorId {
    /// Validates a caller-assigned identity.
    ///
    /// # Errors
    /// Returns `InvalidMotorId` for zero.
    pub const fn new(value: u8) -> Result<Self, InvalidMotorId> {
        if value == 0 {
            Err(InvalidMotorId)
        } else {
            Ok(Self(value))
        }
    }
    /// Returns the one-based identity.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Commanded direction, not a measurement of rotation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotorState {
    /// Logical forwards, before polarity inversion.
    Forward,
    /// Logical backwards, before polarity inversion.
    Backward,
    /// Both outputs inactive; coast rather than active braking.
    Stopped,
}

/// PWM duty in the inclusive range 0..=255, not watts or RPM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Power(u8);
impl Power {
    /// Zero duty.
    pub const ZERO: Self = Self(0);
    /// Full duty.
    pub const FULL: Self = Self(255);
    /// Creates an eight-bit duty command.
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }
    /// Returns the duty command.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
    fn scale(self, maximum: u16) -> u16 {
        // The quotient cannot exceed the u16 maximum supplied by the HAL.
        u16::try_from(u32::from(self.0) * u32::from(maximum) / 255).expect("scaled duty fits u16")
    }
}

/// Output polarity and minimum inactive interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotorConfig {
    inverted: bool,
    commutation_wait_us: u32,
}
/// The commutation/update wait must be nonzero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidConfig;
impl MotorConfig {
    /// Sets polarity and the wait covering timer updates and bridge dead time.
    ///
    /// # Errors
    /// Returns `InvalidConfig` for a zero wait.
    pub const fn new(inverted: bool, commutation_wait_us: u32) -> Result<Self, InvalidConfig> {
        if commutation_wait_us == 0 {
            Err(InvalidConfig)
        } else {
            Ok(Self {
                inverted,
                commutation_wait_us,
            })
        }
    }
    /// Whether logical forward uses input B instead of input A.
    #[must_use]
    pub const fn inverted(self) -> bool {
        self.inverted
    }
    /// Minimum interval with both inputs inactive, in microseconds.
    #[must_use]
    pub const fn commutation_wait_us(self) -> u32 {
        self.commutation_wait_us
    }
}

/// Lifecycle errors and original output failures.
#[derive(Debug, Eq, PartialEq)]
pub enum Error<A, B> {
    /// Initialization is required after construction, failure, or cancelled initialization.
    NotInitialized,
    /// At least one handle reports zero maximum duty; configure its PWM timer first.
    InvalidPwmMaximum,
    /// Select a direction before requesting nonzero power.
    DirectionRequired,
    /// Input A failed. Cleanup was attempted on both inputs.
    OutputA(A),
    /// Input B failed. Cleanup was attempted on both inputs.
    OutputB(B),
}

/// Owns one bridge's independent input PWM handles.
///
/// On an output failure both inputs are best-effort disabled and initialization
/// is invalidated. The original error is returned; cleanup errors are discarded.
/// Physical outputs are unknown after a failed write, even if cleanup succeeds.
/// There is no automatic shutdown on `Drop` or `release`: stop explicitly first.
pub struct Motor<A, B> {
    id: MotorId,
    a: A,
    b: B,
    config: MotorConfig,
    initialized: bool,
    state: Option<MotorState>,
    power: Power,
}
impl<A: SetDutyCycle, B: SetDutyCycle> Motor<A, B> {
    /// Takes ownership without writing outputs. Initialize before commanding.
    #[must_use]
    pub const fn new(id: MotorId, a: A, b: B, config: MotorConfig) -> Self {
        Self {
            id,
            a,
            b,
            config,
            initialized: false,
            state: None,
            power: Power::ZERO,
        }
    }
    /// Returns the caller-assigned identity.
    #[must_use]
    pub const fn id(&self) -> MotorId {
        self.id
    }
    /// Returns the successfully prepared direction, or unknown after failure.
    #[must_use]
    pub const fn state(&self) -> Option<MotorState> {
        self.state
    }
    /// Returns applied duty; unknown output state is indicated separately by `state()`.
    #[must_use]
    pub const fn power(&self) -> Power {
        self.power
    }
    /// Whether initialization completed without a subsequent output failure.
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }
    /// Returns the handles without output I/O. Stop before releasing them.
    #[must_use]
    pub fn release(self) -> (A, B) {
        (self.a, self.b)
    }

    fn failed(&mut self, error: Error<A::Error, B::Error>) -> Error<A::Error, B::Error> {
        self.initialized = false;
        self.state = None;
        self.power = Power::ZERO;
        let _ = self.a.set_duty_cycle(0);
        let _ = self.b.set_duty_cycle(0);
        error
    }
    fn inactive(&mut self) -> Result<(), Error<A::Error, B::Error>> {
        // Disable the previously active leg first, including inverted polarity.
        let b_first = self.state.is_some_and(|s| self.uses_b(s));
        if b_first {
            if let Err(e) = self.b.set_duty_cycle(0) {
                return Err(self.failed(Error::OutputB(e)));
            }
            if let Err(e) = self.a.set_duty_cycle(0) {
                return Err(self.failed(Error::OutputA(e)));
            }
        } else {
            if let Err(e) = self.a.set_duty_cycle(0) {
                return Err(self.failed(Error::OutputA(e)));
            }
            if let Err(e) = self.b.set_duty_cycle(0) {
                return Err(self.failed(Error::OutputB(e)));
            }
        }
        self.state = Some(MotorState::Stopped);
        self.power = Power::ZERO;
        Ok(())
    }
    fn uses_b(&self, state: MotorState) -> bool {
        (state == MotorState::Backward) != self.config.inverted
    }
    fn require_initialized(&self) -> Result<(), Error<A::Error, B::Error>> {
        if self.initialized {
            Ok(())
        } else {
            Err(Error::NotInitialized)
        }
    }
    /// Disables both inputs and waits for timer updates before accepting commands.
    /// Cancellation while waiting leaves initialization incomplete; retry it.
    ///
    /// # Errors
    /// Returns the first PWM error or `InvalidPwmMaximum`; initialization remains invalid.
    pub async fn initialize(
        &mut self,
        delay: &mut impl DelayNs,
    ) -> Result<(), Error<A::Error, B::Error>> {
        self.initialized = false;
        self.inactive()?;
        if self.a.max_duty_cycle() == 0 || self.b.max_duty_cycle() == 0 {
            self.state = None;
            return Err(Error::InvalidPwmMaximum);
        }
        delay.delay_us(self.config.commutation_wait_us).await;
        self.initialized = true;
        Ok(())
    }
    /// Prepares direction. A changed direction clears power and disables both
    /// inputs before waiting. Set power explicitly afterwards. Cancellation after
    /// disable leaves the commanded state stopped; physical disable still depends
    /// on the peripheral's update latency. An unchanged direction retains power.
    ///
    /// # Errors
    /// Returns a lifecycle or PWM error.
    pub async fn set_direction(
        &mut self,
        state: MotorState,
        delay: &mut impl DelayNs,
    ) -> Result<(), Error<A::Error, B::Error>> {
        self.require_initialized()?;
        if state == MotorState::Stopped {
            return self.stop(delay).await;
        }
        if self.state == Some(state) {
            return Ok(());
        }
        self.inactive()?;
        delay.delay_us(self.config.commutation_wait_us).await;
        self.state = Some(state);
        Ok(())
    }
    /// Updates duty without a commutation wait. A stopped motor accepts only zero.
    /// Uses each active handle's maximum duty and floors intermediate scaling.
    ///
    /// # Errors
    /// Returns a lifecycle error, or the original PWM failure after cleanup.
    pub fn set_power(&mut self, power: Power) -> Result<(), Error<A::Error, B::Error>> {
        self.require_initialized()?;
        let state = self.state.unwrap_or(MotorState::Stopped);
        if state == MotorState::Stopped {
            if power != Power::ZERO {
                return Err(Error::DirectionRequired);
            }
            return Ok(());
        }
        if self.uses_b(state) {
            if let Err(e) = self.b.set_duty_cycle(power.scale(self.b.max_duty_cycle())) {
                return Err(self.failed(Error::OutputB(e)));
            }
        } else if let Err(e) = self.a.set_duty_cycle(power.scale(self.a.max_duty_cycle())) {
            return Err(self.failed(Error::OutputA(e)));
        }
        self.power = power;
        Ok(())
    }
    /// Combines direction preparation and duty application. Stopped always clears
    /// power, even if a nonzero power argument was supplied.
    ///
    /// # Errors
    /// Returns a lifecycle or PWM error.
    pub async fn drive(
        &mut self,
        state: MotorState,
        power: Power,
        delay: &mut impl DelayNs,
    ) -> Result<(), Error<A::Error, B::Error>> {
        self.set_direction(state, delay).await?;
        if state == MotorState::Stopped {
            Ok(())
        } else {
            self.set_power(power)
        }
    }
    /// Requests coast, clears power/direction, and waits for the timer update.
    /// May be called after an error as a best-effort shutdown, but does not restore
    /// initialization. Cancellation after disable leaves a stopped command.
    ///
    /// # Errors
    /// Returns the first PWM failure. Failed disablement means outputs are unknown.
    pub async fn stop(
        &mut self,
        delay: &mut impl DelayNs,
    ) -> Result<(), Error<A::Error, B::Error>> {
        self.inactive()?;
        delay.delay_us(self.config.commutation_wait_us).await;
        Ok(())
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

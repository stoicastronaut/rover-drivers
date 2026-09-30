# Two-input brushed DC motor driver

`dc-motor` is an allocation-free `no_std` driver for an H-bridge with two
independent PWM inputs. It owns `embedded_hal::pwm::SetDutyCycle` handles and
borrows an `embedded_hal_async::delay::DelayNs` for transitions. It has no MCU,
Embassy, wheel-position, sensor, or feedback dependency.

The intended interface is low/low for coast, PWM/low for forward, and low/PWM
for backward. It never requests high/high active braking. This matches the
interface of the purchased four-input mini bridge boards; it is not an
L298N enable-pin driver. Confirm the actual chip's truth table and electrical
limits before powering a motor.

## Usage

Configure the MCU PWM frequency, pins, zero/full-duty behavior, and output
update policy in firmware, then pass the two handles to the driver:

```rust
use dc_motor::{Error, Motor, MotorConfig, MotorId, MotorState, Power};
use embedded_hal::pwm::SetDutyCycle;
use embedded_hal_async::delay::DelayNs;

async fn example<A: SetDutyCycle, B: SetDutyCycle>(
    input_a: A,
    input_b: B,
    delay: &mut impl DelayNs,
) -> Result<(A, B), Error<A::Error, B::Error>> {
    // Example only: 2 ms must be checked against the actual peripheral/bridge.
    let config = MotorConfig::new(false, 2_000).unwrap();
    let mut motor = Motor::new(MotorId::new(1).unwrap(), input_a, input_b, config);
    motor.initialize(delay).await?;
    motor.drive(MotorState::Forward, Power::new(64), delay).await?;
    motor.stop(delay).await?;
    // Direction and power calls also compose, as in an Arduino-style API.
    motor.set_direction(MotorState::Backward, delay).await?;
    motor.set_power(Power::new(64))?;
    motor.stop(delay).await?;
    Ok(motor.release())
}
```

`Power` is duty `0..=255`, not measured watts, torque, or RPM. The active
handle receives `floor(power * max_duty_cycle / 255)`; zero and full map to
exact HAL endpoints. Input A and B may have different maxima. A zero maximum
is rejected during initialization. `MotorId` accepts caller-supplied IDs
`1..=255`; wheel numbering belongs to the application. Configuration polarity
swaps physical A/B while retaining logical Forward/Backward state.

## Lifecycle and transitions

Construction performs no output I/O. Initialization disables both inputs and
waits before admitting commands. Initialization can be repeated. A nonzero
power on a stopped motor returns `DirectionRequired`; commands before successful
initialization return `NotInitialized`. Stop clears both direction and power.
`drive(Stopped, power, delay)` always requests zero regardless of its power
argument. Selecting a changed direction disables the previously active input
first, disables the other input, clears power, waits, then prepares the new
direction at zero power. Apply power explicitly after `set_direction`; `drive`
does that for you. An unchanged direction permits a duty update without another
wait. Preparing a direction at zero power retains that selected direction.

The configured nonzero wait, in microseconds, must cover the actual PWM timer's
zero-duty update latency plus bridge dead time. Zero writes can take effect at
a timer boundary. Firmware must verify true zero and full duty on its backend.
Software stop does not promise immediate cessation of physical wheel rotation.

An output failure returns its original `OutputA` or `OutputB` error, attempts
zero on **both** inputs, invalidates initialization, sets `state()` to `None`,
and clears reported duty. Cleanup errors are discarded; physical outputs remain
unknown after failed I/O. Reinitialize before further drive commands. `stop`
can be retried after failure for best-effort shutdown, but cannot restore
initialization. `power()` is meaningful as an applied command only alongside
known state; none of these getters measure physical rotation.

Cancellation of initialization while waiting requires reinitialization.
Cancellation of a direction-change or stop future after successful disablement
leaves a stopped command with zero power; timer update latency still applies.
The delay trait itself is infallible. A failed disablement leaves physical
outputs unknown. Dropping or releasing the driver performs no shutdown; stop
explicitly before releasing handles. Motor power disconnect is a hardware safety
boundary independent of software.

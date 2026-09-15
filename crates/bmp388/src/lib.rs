#![no_std]
#![doc = include_str!("../README.md")]

use embedded_hal_async::{delay::DelayNs, i2c::I2c};

const REG_CHIP_ID: u8 = 0x00;
const REG_ERROR: u8 = 0x02;
const REG_STATUS: u8 = 0x03;
const REG_DATA: u8 = 0x04;
const REG_POWER_CONTROL: u8 = 0x1B;
const REG_OVERSAMPLING: u8 = 0x1C;
const REG_OUTPUT_DATA_RATE: u8 = 0x1D;
const REG_CONFIG: u8 = 0x1F;
const REG_CALIBRATION: u8 = 0x31;
const REG_COMMAND: u8 = 0x7E;

const EXPECTED_CHIP_ID: u8 = 0x50;
const COMMAND_SOFT_RESET: u8 = 0xB6;
const SENSOR_ENABLE_BITS: u8 = 0x03;
const MODE_FORCED_BITS: u8 = 0x10;
const MODE_NORMAL_BITS: u8 = 0x30;

const STARTUP_DELAY_MS: u32 = 2;
const SOFT_RESET_DELAY_MS: u32 = 2;
const MODE_CHANGE_DELAY_MS: u32 = 5;
const POLL_DELAY_MS: u32 = 1;
const COMMAND_READY_ATTEMPTS: usize = 10;
const CALIBRATION_LENGTH: usize = 21;

/// Selects the BMP388 I2C address configured by the SDO pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Address {
    /// SDO tied low.
    Primary = 0x76,
    /// SDO tied high.
    Secondary = 0x77,
}

/// Pressure or temperature oversampling ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Oversampling {
    /// One conversion per sample.
    X1 = 0,
    /// Two conversions per sample.
    X2 = 1,
    /// Four conversions per sample.
    X4 = 2,
    /// Eight conversions per sample.
    X8 = 3,
    /// Sixteen conversions per sample.
    X16 = 4,
    /// Thirty-two conversions per sample.
    X32 = 5,
}

impl Oversampling {
    const fn multiplier(self) -> u32 {
        1 << self as u8
    }
}

/// Output data rate used in normal mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OutputDataRate {
    /// 200 Hz (5 ms period).
    Hz200 = 0,
    /// 100 Hz (10 ms period).
    Hz100 = 1,
    /// 50 Hz (20 ms period).
    Hz50 = 2,
    /// 25 Hz (40 ms period).
    Hz25 = 3,
    /// 12.5 Hz (80 ms period).
    Hz12_5 = 4,
    /// 6.25 Hz (160 ms period).
    Hz6_25 = 5,
    /// Approximately 3.1 Hz (320 ms period).
    Hz3_1 = 6,
    /// Approximately 1.5 Hz (640 ms period).
    Hz1_5 = 7,
}

impl OutputDataRate {
    const fn period_us(self) -> u32 {
        5_000_u32 << self as u8
    }
}

/// Infinite impulse response filter coefficient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IirFilter {
    /// Disable the IIR filter.
    Off = 0,
    /// Filter coefficient 1.
    Coefficient1 = 1,
    /// Filter coefficient 3.
    Coefficient3 = 2,
    /// Filter coefficient 7.
    Coefficient7 = 3,
    /// Filter coefficient 15.
    Coefficient15 = 4,
    /// Filter coefficient 31.
    Coefficient31 = 5,
    /// Filter coefficient 63.
    Coefficient63 = 6,
    /// Filter coefficient 127.
    Coefficient127 = 7,
}

/// Measurement settings applied during initialization or by
/// [`Bmp388::configure`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Pressure oversampling ratio.
    pub pressure_oversampling: Oversampling,
    /// Temperature oversampling ratio.
    pub temperature_oversampling: Oversampling,
    /// Normal-mode output data rate.
    pub output_data_rate: OutputDataRate,
    /// IIR filter setting.
    pub iir_filter: IirFilter,
}

impl Default for Config {
    /// Use 8× pressure oversampling, 2× temperature oversampling, a 25 Hz
    /// output data rate, and an IIR filter coefficient of 3.
    fn default() -> Self {
        Self {
            pressure_oversampling: Oversampling::X8,
            temperature_oversampling: Oversampling::X2,
            output_data_rate: OutputDataRate::Hz25,
            iir_filter: IirFilter::Coefficient3,
        }
    }
}

impl Config {
    const fn oversampling_register(self) -> u8 {
        self.pressure_oversampling as u8 | (self.temperature_oversampling as u8) << 3
    }

    const fn filter_register(self) -> u8 {
        (self.iir_filter as u8) << 1
    }

    const fn measurement_duration_us(self) -> u32 {
        234 + 392
            + self.pressure_oversampling.multiplier() * 2_000
            + 313
            + self.temperature_oversampling.multiplier() * 2_000
    }

    const fn is_valid_for_normal_mode(self) -> bool {
        self.measurement_duration_us() < self.output_data_rate.period_us()
    }
}

/// Sensor error flags read from `ERR_REG`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorErrors {
    /// The sensor reports a fatal internal error.
    pub fatal: bool,
    /// The last command could not be executed.
    pub command: bool,
    /// The sensor rejected a configuration.
    pub configuration: bool,
}

impl SensorErrors {
    const fn from_register(value: u8) -> Self {
        Self {
            fatal: value & (1 << 0) != 0,
            command: value & (1 << 1) != 0,
            configuration: value & (1 << 2) != 0,
        }
    }

    /// Returns `true` when any sensor error flag is set.
    #[must_use]
    pub const fn any(self) -> bool {
        self.fatal || self.command || self.configuration
    }
}

/// Command and measurement readiness flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// The command decoder can accept a command.
    pub command_ready: bool,
    /// A new pressure value is available.
    pub pressure_ready: bool,
    /// A new temperature value is available.
    pub temperature_ready: bool,
}

impl Status {
    const fn from_register(value: u8) -> Self {
        Self {
            command_ready: value & (1 << 4) != 0,
            pressure_ready: value & (1 << 5) != 0,
            temperature_ready: value & (1 << 6) != 0,
        }
    }
}

/// Uncompensated 24-bit pressure and temperature ADC readings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSample {
    /// Raw pressure ADC value.
    pub pressure: u32,
    /// Raw temperature ADC value.
    pub temperature: u32,
}

impl RawSample {
    const fn from_bytes(bytes: [u8; 6]) -> Self {
        Self {
            pressure: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]),
            temperature: u32::from_le_bytes([bytes[3], bytes[4], bytes[5], 0]),
        }
    }
}

/// A compensated BMP388 reading in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measurement {
    /// Barometric pressure in pascals.
    pub pressure_pa: f64,
    /// Temperature in degrees Celsius.
    pub temperature_celsius: f64,
}

/// Errors returned by the BMP388 driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// The underlying I2C transaction failed.
    Bus(E),
    /// The device returned a chip ID other than `0x50`.
    InvalidChipId(u8),
    /// Oversampling and output data rate settings are incompatible.
    InvalidConfig,
    /// An operation requiring factory calibration was called before `init`.
    NotInitialized,
    /// The command decoder did not become ready within the bounded wait.
    CommandReadyTimeout,
    /// Fresh pressure and temperature data did not arrive in time.
    MeasurementTimeout,
    /// The device reported one or more internal error flags.
    SensorFault(SensorErrors),
    /// The requested operation is incompatible with the current power mode.
    InvalidState,
}

#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::struct_field_names,
    reason = "par_t1 through par_p11 are the coefficient names used by Bosch"
)]
struct Calibration {
    par_t1: f64,
    par_t2: f64,
    par_t3: f64,
    par_p1: f64,
    par_p2: f64,
    par_p3: f64,
    par_p4: f64,
    par_p5: f64,
    par_p6: f64,
    par_p7: f64,
    par_p8: f64,
    par_p9: f64,
    par_p10: f64,
    par_p11: f64,
}

impl Calibration {
    fn from_bytes(bytes: [u8; CALIBRATION_LENGTH]) -> Self {
        let t1 = u16::from_le_bytes([bytes[0], bytes[1]]);
        let t2 = u16::from_le_bytes([bytes[2], bytes[3]]);
        let t3 = i8::from_ne_bytes([bytes[4]]);
        let p1 = i16::from_le_bytes([bytes[5], bytes[6]]);
        let p2 = i16::from_le_bytes([bytes[7], bytes[8]]);
        let p3 = i8::from_ne_bytes([bytes[9]]);
        let p4 = i8::from_ne_bytes([bytes[10]]);
        let p5 = u16::from_le_bytes([bytes[11], bytes[12]]);
        let p6 = u16::from_le_bytes([bytes[13], bytes[14]]);
        let p7 = i8::from_ne_bytes([bytes[15]]);
        let p8 = i8::from_ne_bytes([bytes[16]]);
        let p9 = i16::from_le_bytes([bytes[17], bytes[18]]);
        let p10 = i8::from_ne_bytes([bytes[19]]);
        let p11 = i8::from_ne_bytes([bytes[20]]);

        Self {
            par_t1: f64::from(t1) * 256.0,
            par_t2: f64::from(t2) / 1_073_741_824.0,
            par_t3: f64::from(t3) / 281_474_976_710_656.0,
            par_p1: (f64::from(p1) - 16_384.0) / 1_048_576.0,
            par_p2: (f64::from(p2) - 16_384.0) / 536_870_912.0,
            par_p3: f64::from(p3) / 4_294_967_296.0,
            par_p4: f64::from(p4) / 137_438_953_472.0,
            par_p5: f64::from(p5) * 8.0,
            par_p6: f64::from(p6) / 64.0,
            par_p7: f64::from(p7) / 256.0,
            par_p8: f64::from(p8) / 32_768.0,
            par_p9: f64::from(p9) / 281_474_976_710_656.0,
            par_p10: f64::from(p10) / 281_474_976_710_656.0,
            par_p11: f64::from(p11) / 36_893_488_147_419_103_232.0,
        }
    }

    fn compensate(self, raw: RawSample) -> Measurement {
        let partial_temperature = f64::from(raw.temperature) - self.par_t1;
        let t_lin = partial_temperature * self.par_t2
            + partial_temperature * partial_temperature * self.par_t3;
        let t2 = t_lin * t_lin;
        let t3 = t2 * t_lin;
        let pressure = f64::from(raw.pressure);
        let p2 = pressure * pressure;
        let p3 = p2 * pressure;

        let offset = self.par_p5 + self.par_p6 * t_lin + self.par_p7 * t2 + self.par_p8 * t3;
        let sensitivity =
            pressure * (self.par_p1 + self.par_p2 * t_lin + self.par_p3 * t2 + self.par_p4 * t3);
        let nonlinearity = p2 * (self.par_p9 + self.par_p10 * t_lin) + p3 * self.par_p11;

        Measurement {
            pressure_pa: (offset + sensitivity + nonlinearity).clamp(30_000.0, 125_000.0),
            temperature_celsius: t_lin.clamp(-40.0, 85.0),
        }
    }
}

/// BMP388 driver using an asynchronous I2C bus.
pub struct Bmp388<I2C> {
    i2c: I2C,
    address: Address,
    config: Config,
    calibration: Option<Calibration>,
    normal_mode: bool,
}

impl<I2C> Bmp388<I2C> {
    /// Creates a driver with [`Config::default`] without accessing the bus.
    #[must_use]
    pub fn new(i2c: I2C, address: Address) -> Self {
        Self::with_config(i2c, address, Config::default())
    }

    /// Creates a driver with explicit settings without accessing the bus.
    #[must_use]
    pub const fn with_config(i2c: I2C, address: Address, config: Config) -> Self {
        Self {
            i2c,
            address,
            config,
            calibration: None,
            normal_mode: false,
        }
    }

    /// Returns the currently selected settings.
    #[must_use]
    pub const fn config(&self) -> Config {
        self.config
    }

    /// Consumes the driver and returns its I2C bus handle.
    #[must_use]
    pub fn release(self) -> I2C {
        self.i2c
    }
}

impl<I2C> Bmp388<I2C>
where
    I2C: I2c,
{
    /// Initializes the device, loads calibration, and applies configuration.
    ///
    /// The sensor remains in sleep mode. Use [`Self::measure_forced`] for
    /// one-shot measurements or [`Self::start_normal_mode`] for continuous
    /// conversion.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for bus failures, an unexpected device identity,
    /// reset/readiness failures, sensor faults, or invalid configuration.
    pub async fn init<D>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>>
    where
        D: DelayNs,
    {
        delay.delay_ms(STARTUP_DELAY_MS).await;
        self.verify_identity().await?;
        self.soft_reset(delay).await?;
        self.calibration = Some(self.read_calibration().await?);
        self.configure(self.config).await
    }

    /// Reads the device identity register.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] if the I2C transaction fails.
    pub async fn chip_id(&mut self) -> Result<u8, Error<I2C::Error>> {
        self.read_register(REG_CHIP_ID).await
    }

    /// Verifies that the connected device reports the BMP388 chip ID.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidChipId`] for another device or [`Error::Bus`]
    /// if the I2C transaction fails.
    pub async fn verify_identity(&mut self) -> Result<(), Error<I2C::Error>> {
        let chip_id = self.chip_id().await?;
        if chip_id == EXPECTED_CHIP_ID {
            Ok(())
        } else {
            Err(Error::InvalidChipId(chip_id))
        }
    }

    /// Reads command and measurement readiness flags.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] if the I2C transaction fails.
    pub async fn status(&mut self) -> Result<Status, Error<I2C::Error>> {
        let value = self.read_register(REG_STATUS).await?;
        Ok(Status::from_register(value))
    }

    /// Reads the sensor's internal error flags.
    ///
    /// Command and configuration flags are cleared by this read.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] if the I2C transaction fails.
    pub async fn error_status(&mut self) -> Result<SensorErrors, Error<I2C::Error>> {
        let value = self.read_register(REG_ERROR).await?;
        Ok(SensorErrors::from_register(value))
    }

    /// Performs a soft reset and waits for the command decoder to recover.
    ///
    /// A successful reset invalidates previously loaded calibration. Call
    /// [`Self::init`] before requesting another compensated measurement.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for bus failures, readiness timeout, or a sensor
    /// error reported after reset.
    pub async fn soft_reset<D>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>>
    where
        D: DelayNs,
    {
        self.wait_for_command_ready(delay).await?;
        self.write_register(REG_COMMAND, COMMAND_SOFT_RESET).await?;
        self.calibration = None;
        self.normal_mode = false;
        delay.delay_ms(SOFT_RESET_DELAY_MS).await;
        self.ensure_no_sensor_errors().await?;
        self.wait_for_command_ready(delay).await
    }

    /// Applies pressure, temperature, data-rate, and filter settings.
    ///
    /// The sensor is left in sleep mode. Stop normal mode before reconfiguring.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] when measurement time does not fit the
    /// configured normal-mode period, [`Error::InvalidState`] while normal mode
    /// is active, or an error from the device or bus.
    pub async fn configure(&mut self, config: Config) -> Result<(), Error<I2C::Error>> {
        if self.normal_mode {
            return Err(Error::InvalidState);
        }
        if !config.is_valid_for_normal_mode() {
            return Err(Error::InvalidConfig);
        }

        self.write_register(REG_POWER_CONTROL, SENSOR_ENABLE_BITS)
            .await?;
        self.write_register(REG_OVERSAMPLING, config.oversampling_register())
            .await?;
        self.write_register(REG_OUTPUT_DATA_RATE, config.output_data_rate as u8)
            .await?;
        self.write_register(REG_CONFIG, config.filter_register())
            .await?;
        self.ensure_no_sensor_errors().await?;
        self.config = config;
        Ok(())
    }

    /// Reads one complete uncompensated pressure and temperature burst.
    ///
    /// This advanced API does not guarantee freshness and does not require
    /// initialization.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] if the I2C transaction fails.
    pub async fn read_raw(&mut self) -> Result<RawSample, Error<I2C::Error>> {
        let mut data = [0_u8; 6];
        self.read_registers(REG_DATA, &mut data).await?;
        Ok(RawSample::from_bytes(data))
    }

    /// Reads and compensates the values currently held in the data registers.
    ///
    /// This method does not wait for fresh data. Prefer [`Self::measure_forced`]
    /// or [`Self::read_normal_measurement`] when freshness matters.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotInitialized`] if calibration is unavailable or
    /// [`Error::Bus`] if the data read fails.
    pub async fn read_latest(&mut self) -> Result<Measurement, Error<I2C::Error>> {
        let calibration = self.calibration.ok_or(Error::NotInitialized)?;
        let raw = self.read_raw().await?;
        Ok(calibration.compensate(raw))
    }

    /// Triggers one forced conversion and returns a fresh compensated sample.
    ///
    /// If normal mode is active, it is stopped before starting the forced
    /// conversion. The sensor returns to sleep automatically afterward.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if initialization has not completed, the bus fails,
    /// sensor data does not become ready, or the sensor reports a fault.
    pub async fn measure_forced<D>(
        &mut self,
        delay: &mut D,
    ) -> Result<Measurement, Error<I2C::Error>>
    where
        D: DelayNs,
    {
        if self.calibration.is_none() {
            return Err(Error::NotInitialized);
        }
        if self.normal_mode {
            self.stop_normal_mode().await?;
            delay.delay_ms(MODE_CHANGE_DELAY_MS).await;
        }

        self.write_register(REG_POWER_CONTROL, SENSOR_ENABLE_BITS | MODE_FORCED_BITS)
            .await?;
        self.normal_mode = false;
        self.wait_for_data_ready(delay, self.forced_poll_attempts())
            .await?;
        self.read_latest().await
    }

    /// Starts continuous normal-mode conversion with the current settings.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotInitialized`] before [`Self::init`], or a sensor or
    /// bus error while changing mode.
    pub async fn start_normal_mode(&mut self) -> Result<(), Error<I2C::Error>> {
        if self.calibration.is_none() {
            return Err(Error::NotInitialized);
        }
        self.write_register(REG_POWER_CONTROL, SENSOR_ENABLE_BITS | MODE_NORMAL_BITS)
            .await?;
        self.ensure_no_sensor_errors().await?;
        self.normal_mode = true;
        Ok(())
    }

    /// Stops continuous conversion and leaves both sensing channels enabled.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] if the mode write fails.
    pub async fn stop_normal_mode(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_register(REG_POWER_CONTROL, SENSOR_ENABLE_BITS)
            .await?;
        self.normal_mode = false;
        Ok(())
    }

    /// Waits for and reads a fresh sample in normal mode.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidState`] unless normal mode is active, or an
    /// initialization, timeout, sensor, or bus error.
    pub async fn read_normal_measurement<D>(
        &mut self,
        delay: &mut D,
    ) -> Result<Measurement, Error<I2C::Error>>
    where
        D: DelayNs,
    {
        if !self.normal_mode {
            return Err(Error::InvalidState);
        }

        let attempts = usize::try_from(self.config.output_data_rate.period_us() / 1_000)
            .unwrap_or(usize::MAX)
            .saturating_add(2);
        self.wait_for_data_ready(delay, attempts).await?;
        self.read_latest().await
    }

    async fn read_register(&mut self, register: u8) -> Result<u8, Error<I2C::Error>> {
        let mut value = [0_u8; 1];
        self.read_registers(register, &mut value).await?;
        Ok(value[0])
    }

    async fn read_registers(
        &mut self,
        start_register: u8,
        buffer: &mut [u8],
    ) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write_read(self.address as u8, &[start_register], buffer)
            .await
            .map_err(Error::Bus)
    }

    async fn write_register(&mut self, register: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.address as u8, &[register, value])
            .await
            .map_err(Error::Bus)
    }

    async fn read_calibration(&mut self) -> Result<Calibration, Error<I2C::Error>> {
        let mut bytes = [0_u8; CALIBRATION_LENGTH];
        self.read_registers(REG_CALIBRATION, &mut bytes).await?;
        Ok(Calibration::from_bytes(bytes))
    }

    async fn ensure_no_sensor_errors(&mut self) -> Result<(), Error<I2C::Error>> {
        let errors = self.error_status().await?;
        if errors.any() {
            Err(Error::SensorFault(errors))
        } else {
            Ok(())
        }
    }

    async fn wait_for_command_ready<D>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>>
    where
        D: DelayNs,
    {
        for attempt in 0..COMMAND_READY_ATTEMPTS {
            if self.status().await?.command_ready {
                return Ok(());
            }
            if attempt + 1 < COMMAND_READY_ATTEMPTS {
                delay.delay_ms(POLL_DELAY_MS).await;
            }
        }
        Err(Error::CommandReadyTimeout)
    }

    async fn wait_for_data_ready<D>(
        &mut self,
        delay: &mut D,
        attempts: usize,
    ) -> Result<(), Error<I2C::Error>>
    where
        D: DelayNs,
    {
        for attempt in 0..attempts {
            let status = self.status().await?;
            if status.pressure_ready && status.temperature_ready {
                return Ok(());
            }
            if attempt + 1 < attempts {
                delay.delay_ms(POLL_DELAY_MS).await;
            }
        }

        self.ensure_no_sensor_errors().await?;
        Err(Error::MeasurementTimeout)
    }

    fn forced_poll_attempts(&self) -> usize {
        usize::try_from(self.config.measurement_duration_us().div_ceil(1_000))
            .unwrap_or(usize::MAX)
            .saturating_add(2)
    }
}

#[cfg(test)]
mod tests;

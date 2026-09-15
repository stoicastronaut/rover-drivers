#![no_std]
#![doc = include_str!("../README.md")]

use embedded_hal_async::{delay::DelayNs, i2c::I2c};

// Nominal sensitivity factors from TDK DS-000189, sections 3.1–3.3.
const ACCEL_COUNTS_PER_G_2G: f32 = 16_384.0;
const ACCEL_COUNTS_PER_G_4G: f32 = 8_192.0;
const ACCEL_COUNTS_PER_G_8G: f32 = 4_096.0;
const ACCEL_COUNTS_PER_G_16G: f32 = 2_048.0;
const GYRO_COUNTS_PER_DEGREE_S_250DPS: f32 = 131.0;
const GYRO_COUNTS_PER_DEGREE_S_500DPS: f32 = 65.5;
const GYRO_COUNTS_PER_DEGREE_S_1000DPS: f32 = 32.8;
const GYRO_COUNTS_PER_DEGREE_S_2000DPS: f32 = 16.4;
const MAGNETIC_MICROTESLA_PER_COUNT: f32 = 0.15;

/// Standard acceleration of gravity in m/s², used to convert g to SI units.
const STANDARD_GRAVITY_M_S2: f32 = 9.806_65;
// Die temperature conversion from TDK DS-000189, sections 3.4 and 8.31.
const TEMPERATURE_COUNTS_PER_DEGREE_C: f32 = 333.87;
const TEMPERATURE_ZERO_COUNT_C: f32 = 21.0;

// TDK DS-000189, sections 8, 10, 12 and 13. Bank selection is never cached.
const REG_BANK_SEL: u8 = 0x7f;
const REG_WHO_AM_I: u8 = 0x00;
const REG_USER_CTRL: u8 = 0x03;
const REG_LP_CONFIG: u8 = 0x05;
const REG_PWR_MGMT_1: u8 = 0x06;
const REG_PWR_MGMT_2: u8 = 0x07;
const REG_INT_PIN_CFG: u8 = 0x0f;
const REG_ACCEL_XOUT_H: u8 = 0x2d;
const REG_GYRO_SMPLRT_DIV: u8 = 0x00;
const REG_GYRO_CONFIG_1: u8 = 0x01;
const REG_ACCEL_SMPLRT_DIV_1: u8 = 0x10;
const REG_ACCEL_SMPLRT_DIV_2: u8 = 0x11;
const REG_ACCEL_CONFIG: u8 = 0x14;
const MAG_ADDRESS: u8 = 0x0c;
const REG_MAG_WIA2: u8 = 0x01;
const REG_MAG_ST1: u8 = 0x10;
const REG_MAG_CNTL2: u8 = 0x31;
const REG_MAG_CNTL3: u8 = 0x32;

/// ICM-20948 host address selected by AD0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Address {
    /// AD0 low: 0x68.
    Primary = 0x68,
    /// AD0 high: 0x69.
    Secondary = 0x69,
}

/// Accelerometer full-scale range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AccelRange {
    /// ±2 g.
    G2 = 0,
    /// ±4 g.
    G4 = 1,
    /// ±8 g.
    G8 = 2,
    /// ±16 g.
    G16 = 3,
}

/// Gyroscope full-scale range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GyroRange {
    /// ±250 degrees/s.
    Dps250 = 0,
    /// ±500 degrees/s.
    Dps500 = 1,
    /// ±1000 degrees/s.
    Dps1000 = 2,
    /// ±2000 degrees/s.
    Dps2000 = 3,
}

/// Accelerometer digital low-pass filter (3 dB bandwidth).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AccelDlpf {
    /// 246 Hz.
    Hz246 = 1,
    /// 111.4 Hz.
    Hz111_4 = 2,
    /// 50.4 Hz.
    Hz50_4 = 3,
    /// 23.9 Hz.
    Hz23_9 = 4,
    /// 11.5 Hz.
    Hz11_5 = 5,
    /// 5.7 Hz.
    Hz5_7 = 6,
    /// 473 Hz.
    Hz473 = 7,
}

/// Gyroscope digital low-pass filter (3 dB bandwidth).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GyroDlpf {
    /// 151.8 Hz.
    Hz151_8 = 1,
    /// 119.5 Hz.
    Hz119_5 = 2,
    /// 51.2 Hz.
    Hz51_2 = 3,
    /// 23.9 Hz.
    Hz23_9 = 4,
    /// 11.6 Hz.
    Hz11_6 = 5,
    /// 5.7 Hz.
    Hz5_7 = 6,
}

/// AK09916 continuous measurement rate, or disabled bypass access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MagnetometerMode {
    /// Do not expose or configure the magnetometer.
    Disabled = 0,
    /// 10 Hz.
    Hz10 = 2,
    /// 20 Hz.
    Hz20 = 4,
    /// 50 Hz.
    Hz50 = 6,
    /// 100 Hz.
    Hz100 = 8,
}

/// Configuration applied by every initialization; filters are always enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// Accelerometer full-scale range.
    pub accel_range: AccelRange,
    /// Gyroscope full-scale range.
    pub gyro_range: GyroRange,
    /// Accelerometer low-pass filter.
    pub accel_dlpf: AccelDlpf,
    /// Gyroscope low-pass filter.
    pub gyro_dlpf: GyroDlpf,
    /// Accelerometer rate is nominally 1125 / (1 + divider) Hz; maximum 4095.
    pub accel_sample_rate_divider: u16,
    /// Gyroscope rate is nominally 1125 / (1 + divider) Hz.
    pub gyro_sample_rate_divider: u8,
    /// Magnetometer rate; enabled modes require exclusive use of address 0x0C.
    pub magnetometer: MagnetometerMode,
}

impl Default for Config {
    /// Use ±4 g acceleration and ±500 °/s angular velocity ranges, with
    /// 50.4 Hz accelerometer and 51.2 Hz gyroscope low-pass filters.
    /// Both sample-rate dividers are 4 (nominally 225 Hz), and the
    /// magnetometer runs at 100 Hz through I2C bypass.
    fn default() -> Self {
        Self {
            accel_range: AccelRange::G4,
            gyro_range: GyroRange::Dps500,
            accel_dlpf: AccelDlpf::Hz50_4,
            gyro_dlpf: GyroDlpf::Hz51_2,
            accel_sample_rate_divider: 4,
            gyro_sample_rate_divider: 4,
            magnetometer: MagnetometerMode::Hz100,
        }
    }
}

/// Latest accelerometer, gyroscope and temperature registers, decoded as signed counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSample {
    /// Accelerometer X, Y, Z in native sensor axes.
    pub accel: [i16; 3],
    /// Gyroscope X, Y, Z in native sensor axes.
    pub gyro: [i16; 3],
    /// Die temperature counts.
    pub temperature: i16,
}

/// Inertial reading in physical units; no calibration or axis remapping is applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Acceleration X, Y, Z in m/s², including gravity.
    pub accel_m_s2: [f32; 3],
    /// Angular velocity X, Y, Z in radians/s.
    pub gyro_rad_s: [f32; 3],
    /// Die temperature in °C.
    pub temperature_c: f32,
}

/// Valid AK09916 reading in its native axes, which differ from the inertial axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawMagneticSample {
    /// Magnetic X, Y, Z counts; each count is 0.15 µT.
    pub magnetic: [i16; 3],
    /// True if at least one measurement was skipped before this reading.
    pub overrun: bool,
}

/// Magnetic reading without hard/soft-iron compensation or axis remapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagneticSample {
    /// Magnetic flux density X, Y, Z in µT, in AK09916 axes.
    pub magnetic_ut: [f32; 3],
    /// True if at least one measurement was skipped before this reading.
    pub overrun: bool,
}

/// Transport, identity, configuration and lifecycle errors.
#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// Original HAL error from either I2C address.
    Bus(E),
    /// ICM-20948 identity differed from 0xEA.
    InvalidWhoAmI(u8),
    /// AK09916 identity differed from 0x09.
    InvalidMagnetometerWhoAmI(u8),
    /// Accelerometer divider exceeded 4095.
    InvalidConfig,
    /// Complete initialization is required before reading samples.
    NotInitialized,
    /// Magnetometer was disabled in the configuration.
    MagnetometerDisabled,
    /// AK09916 reported overflow; this measurement was consumed and discarded.
    MagneticOverflow,
}

/// Async I2C driver owning its bus handle and immutable configuration.
pub struct Icm20948<I2C> {
    i2c: I2C,
    address: Address,
    config: Config,
    initialized: bool,
}

impl<I2C> Icm20948<I2C> {
    /// Construct without I/O. Call [`Self::init`] before reading samples.
    #[must_use]
    pub const fn new(i2c: I2C, address: Address, config: Config) -> Self {
        Self {
            i2c,
            address,
            config,
            initialized: false,
        }
    }

    /// Return the owned bus without changing the hardware.
    #[must_use]
    pub fn release(self) -> I2C {
        self.i2c
    }
}

impl<I2C: I2c> Icm20948<I2C> {
    /// Reset, verify and configure the device in continuous low-noise mode.
    ///
    /// Waits 100 ms before I/O, 100 ms after reset, 1 ms after magnetometer
    /// reset (if enabled), and 100 ms after configuration for sensor startup.
    /// Failed or cancelled initialization must be retried in full.
    ///
    /// # Errors
    /// Returns configuration, identity or bus errors; sample reads remain blocked.
    pub async fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Error<I2C::Error>> {
        self.initialized = false;
        if self.config.accel_sample_rate_divider > 4095 {
            return Err(Error::InvalidConfig);
        }
        delay.delay_ms(100).await;
        let identity = self.who_am_i().await?;
        if identity != 0xea {
            return Err(Error::InvalidWhoAmI(identity));
        }
        self.write_register(REG_PWR_MGMT_1, 0x80).await?;
        delay.delay_ms(100).await;
        self.write_register(REG_BANK_SEL, 0).await?;
        self.write_register(REG_PWR_MGMT_1, 0x01).await?;
        self.write_register(REG_PWR_MGMT_2, 0).await?;
        self.write_register(REG_LP_CONFIG, 0).await?;
        self.write_register(REG_USER_CTRL, 0).await?;
        self.write_register(REG_BANK_SEL, 0x20).await?;
        self.write_register(REG_GYRO_SMPLRT_DIV, self.config.gyro_sample_rate_divider)
            .await?;
        self.write_register(
            REG_GYRO_CONFIG_1,
            ((self.config.gyro_dlpf as u8) << 3) | ((self.config.gyro_range as u8) << 1) | 1,
        )
        .await?;
        let [high, low] = self.config.accel_sample_rate_divider.to_be_bytes();
        self.write_register(REG_ACCEL_SMPLRT_DIV_1, high).await?;
        self.write_register(REG_ACCEL_SMPLRT_DIV_2, low).await?;
        self.write_register(
            REG_ACCEL_CONFIG,
            ((self.config.accel_dlpf as u8) << 3) | ((self.config.accel_range as u8) << 1) | 1,
        )
        .await?;
        self.write_register(REG_BANK_SEL, 0).await?;
        let mag_enabled = self.config.magnetometer != MagnetometerMode::Disabled;
        self.write_register(REG_INT_PIN_CFG, if mag_enabled { 0x02 } else { 0 })
            .await?;
        if mag_enabled {
            self.i2c
                .write(MAG_ADDRESS, &[REG_MAG_CNTL3, 1])
                .await
                .map_err(Error::Bus)?;
            delay.delay_ms(1).await;
            let mut identity = [0];
            self.i2c
                .write_read(MAG_ADDRESS, &[REG_MAG_WIA2], &mut identity)
                .await
                .map_err(Error::Bus)?;
            if identity[0] != 0x09 {
                return Err(Error::InvalidMagnetometerWhoAmI(identity[0]));
            }
            self.i2c
                .write(
                    MAG_ADDRESS,
                    &[REG_MAG_CNTL2, self.config.magnetometer as u8],
                )
                .await
                .map_err(Error::Bus)?;
        }
        delay.delay_ms(100).await;
        self.initialized = true;
        Ok(())
    }

    /// Select bank zero and read identity; does not initialize or reset the device.
    /// The caller must allow power-up time when calling this before `init`.
    ///
    /// # Errors
    /// Returns [`Error::Bus`] if bank selection or the read fails.
    pub async fn who_am_i(&mut self) -> Result<u8, Error<I2C::Error>> {
        self.write_register(REG_BANK_SEL, 0).await?;
        let mut value = [0];
        self.i2c
            .write_read(self.address as u8, &[REG_WHO_AM_I], &mut value)
            .await
            .map_err(Error::Bus)?;
        Ok(value[0])
    }

    /// Select bank zero and read the latest 14-byte inertial/temperature burst.
    /// Does not wait for fresh data. Failed/cancelled reads may be retried.
    ///
    /// # Errors
    /// Returns [`Error::NotInitialized`] or [`Error::Bus`].
    pub async fn read_raw(&mut self) -> Result<RawSample, Error<I2C::Error>> {
        self.require_initialized()?;
        self.write_register(REG_BANK_SEL, 0).await?;
        let mut bytes = [0; 14];
        self.i2c
            .write_read(self.address as u8, &[REG_ACCEL_XOUT_H], &mut bytes)
            .await
            .map_err(Error::Bus)?;
        Ok(RawSample {
            accel: [be_word(&bytes, 0), be_word(&bytes, 2), be_word(&bytes, 4)],
            gyro: [be_word(&bytes, 6), be_word(&bytes, 8), be_word(&bytes, 10)],
            temperature: be_word(&bytes, 12),
        })
    }

    /// Read the latest inertial burst and convert to m/s², rad/s and °C.
    ///
    /// # Errors
    /// Returns [`Error::NotInitialized`] or [`Error::Bus`].
    pub async fn read_sample(&mut self) -> Result<Sample, Error<I2C::Error>> {
        let raw = self.read_raw().await?;
        let accel_counts_per_g = match self.config.accel_range {
            AccelRange::G2 => ACCEL_COUNTS_PER_G_2G,
            AccelRange::G4 => ACCEL_COUNTS_PER_G_4G,
            AccelRange::G8 => ACCEL_COUNTS_PER_G_8G,
            AccelRange::G16 => ACCEL_COUNTS_PER_G_16G,
        };
        let gyro_counts_per_degree_s = match self.config.gyro_range {
            GyroRange::Dps250 => GYRO_COUNTS_PER_DEGREE_S_250DPS,
            GyroRange::Dps500 => GYRO_COUNTS_PER_DEGREE_S_500DPS,
            GyroRange::Dps1000 => GYRO_COUNTS_PER_DEGREE_S_1000DPS,
            GyroRange::Dps2000 => GYRO_COUNTS_PER_DEGREE_S_2000DPS,
        };
        Ok(Sample {
            accel_m_s2: raw
                .accel
                .map(|v| f32::from(v) * (STANDARD_GRAVITY_M_S2 / accel_counts_per_g)),
            gyro_rad_s: raw
                .gyro
                .map(|v| (f32::from(v) / gyro_counts_per_degree_s).to_radians()),
            temperature_c: f32::from(raw.temperature) / TEMPERATURE_COUNTS_PER_DEGREE_C
                + TEMPERATURE_ZERO_COUNT_C,
        })
    }

    /// Read AK09916 ST1 through ST2, including the byte at 0x17, in one burst.
    /// Returns `None` when ST1 indicates no new data. ST2 is always read to end
    /// the transfer. A failed/cancelled read can lose a sample; retry a full read
    /// once the HAL bus is usable to release any pending hardware data latch.
    ///
    /// # Errors
    /// Returns lifecycle, disabled magnetometer, bus or magnetic overflow errors.
    pub async fn read_magnetic_raw(
        &mut self,
    ) -> Result<Option<RawMagneticSample>, Error<I2C::Error>> {
        self.require_initialized()?;
        if self.config.magnetometer == MagnetometerMode::Disabled {
            return Err(Error::MagnetometerDisabled);
        }
        let mut bytes = [0; 9];
        self.i2c
            .write_read(MAG_ADDRESS, &[REG_MAG_ST1], &mut bytes)
            .await
            .map_err(Error::Bus)?;
        if bytes[0] & 1 == 0 {
            return Ok(None);
        }
        if bytes[8] & 8 != 0 {
            return Err(Error::MagneticOverflow);
        }
        Ok(Some(RawMagneticSample {
            magnetic: [
                i16::from_le_bytes([bytes[1], bytes[2]]),
                i16::from_le_bytes([bytes[3], bytes[4]]),
                i16::from_le_bytes([bytes[5], bytes[6]]),
            ],
            overrun: bytes[0] & 2 != 0,
        }))
    }

    /// Read a fresh magnetic sample in µT, or `None` if none is ready.
    ///
    /// # Errors
    /// Returns the same errors as [`Self::read_magnetic_raw`].
    pub async fn read_magnetic_sample(
        &mut self,
    ) -> Result<Option<MagneticSample>, Error<I2C::Error>> {
        Ok(self.read_magnetic_raw().await?.map(|raw| MagneticSample {
            magnetic_ut: raw
                .magnetic
                .map(|v| f32::from(v) * MAGNETIC_MICROTESLA_PER_COUNT),
            overrun: raw.overrun,
        }))
    }

    fn require_initialized(&self) -> Result<(), Error<I2C::Error>> {
        if self.initialized {
            Ok(())
        } else {
            Err(Error::NotInitialized)
        }
    }

    async fn write_register(&mut self, register: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.address as u8, &[register, value])
            .await
            .map_err(Error::Bus)
    }
}

fn be_word(bytes: &[u8; 14], offset: usize) -> i16 {
    i16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

#[cfg(test)]
mod tests;

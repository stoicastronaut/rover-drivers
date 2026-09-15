#![no_std]
#![doc = include_str!("../README.md")]

use embedded_hal_async::i2c::I2c;

const STANDARD_GRAVITY_M_S2: f32 = 9.806_65;

// Frequency divider; 1kHz -> 200Hz
const REG_SMPLRT_DIV: u8 = 0x19;

// Enables the 44Hz Low-pass filtering
const REG_CONFIG: u8 = 0x1A;

// Gyro Config -> +-500°/s
const REG_GYRO_CONFIG: u8 = 0x1B;

// Select+-4g
const REG_ACCEL_CONFIG: u8 = 0x1C;
const REG_ACCEL_XOUT_H: u8 = 0x3B;
const REG_PWR_MGMT_1: u8 = 0x6B;

// Confirms the MPU6050
const REG_WHO_AM_I: u8 = 0x75;

const WHO_AM_I_MPU6050: u8 = 0x68;
// Identity reported by the MPU6050-compatible clone used by this project.
const WHO_AM_I_COMPATIBLE_CLONE: u8 = 0x70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Address {
    Primary = 0x68,
    Secondary = 0x69,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccelRange {
    G2,
    G4,
    G8,
    G16,
}

impl AccelRange {
    const fn register_bits(self) -> u8 {
        match self {
            Self::G2 => 0,
            Self::G4 => 1 << 3,
            Self::G8 => 2 << 3,
            Self::G16 => 3 << 3,
        }
    }

    const fn lsb_per_g(self) -> f32 {
        match self {
            Self::G2 => 16_384.0,
            Self::G4 => 8_192.0,
            Self::G8 => 4_096.0,
            Self::G16 => 2_048.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GyroRange {
    Dps250,
    Dps500,
    Dps1000,
    Dps2000,
}

impl GyroRange {
    const fn register_bits(self) -> u8 {
        match self {
            Self::Dps250 => 0,
            Self::Dps500 => 1 << 3,
            Self::Dps1000 => 2 << 3,
            Self::Dps2000 => 3 << 3,
        }
    }

    const fn lsb_per_degree_s(self) -> f32 {
        match self {
            Self::Dps250 => 131.0,
            Self::Dps500 => 65.5,
            Self::Dps1000 => 32.8,
            Self::Dps2000 => 16.4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Dlpf {
    Hz260 = 0,
    Hz184 = 1,
    Hz94 = 2,
    Hz44 = 3,
    Hz21 = 4,
    Hz10 = 5,
    Hz5 = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub accel_range: AccelRange,
    pub gyro_range: GyroRange,
    pub dlpf: Dlpf,
    pub sample_rate_hz: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            accel_range: AccelRange::G4,
            gyro_range: GyroRange::Dps500,
            dlpf: Dlpf::Hz44,
            sample_rate_hz: 200,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSample {
    pub accel: [i16; 3],
    pub temperature: i16,
    pub gyro: [i16; 3],
}

/// One MPU6050 reading converted to physical units while preserving sensor axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub accel_m_s2: [f32; 3],
    pub gyro_rad_s: [f32; 3],
    pub temperature_c: f32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Bus(E),
    InvalidWhoAmI(u8),
    InvalidConfig,
}

pub struct Mpu6050<I2C> {
    i2c: I2C,
    address: Address,
    config: Config,
}

impl<I2C> Mpu6050<I2C> {
    #[must_use]
    pub const fn new(i2c: I2C, address: Address, config: Config) -> Self {
        Self {
            i2c,
            address,
            config,
        }
    }

    #[must_use]
    pub fn release(self) -> I2C {
        self.i2c
    }
}

impl<I2C> Mpu6050<I2C>
where
    I2C: I2c,
{
    /// Validate the device identity and apply the configured ranges and rate.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for invalid configuration, an unexpected identity,
    /// or an I2C transaction failure.
    pub async fn init(&mut self) -> Result<(), Error<I2C::Error>> {
        let sample_divider = sample_rate_divider(self.config)?;
        let identity = self.who_am_i().await?;
        if identity != WHO_AM_I_MPU6050 && identity != WHO_AM_I_COMPATIBLE_CLONE {
            return Err(Error::InvalidWhoAmI(identity));
        }

        self.write_register(REG_PWR_MGMT_1, 0x01).await?;
        self.write_register(REG_CONFIG, self.config.dlpf as u8)
            .await?;
        self.write_register(REG_SMPLRT_DIV, sample_divider).await?;
        self.write_register(REG_GYRO_CONFIG, self.config.gyro_range.register_bits())
            .await?;
        self.write_register(REG_ACCEL_CONFIG, self.config.accel_range.register_bits())
            .await
    }

    /// Read the device identity register.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] when the I2C transaction fails.
    pub async fn who_am_i(&mut self) -> Result<u8, Error<I2C::Error>> {
        let mut value = [0_u8; 1];
        self.i2c
            .write_read(self.address as u8, &[REG_WHO_AM_I], &mut value)
            .await
            .map_err(Error::Bus)?;
        Ok(value[0])
    }

    /// Read one complete raw accelerometer, temperature, and gyroscope burst.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] when the I2C transaction fails.
    pub async fn read_raw(&mut self) -> Result<RawSample, Error<I2C::Error>> {
        let mut bytes = [0_u8; 14];
        self.i2c
            .write_read(self.address as u8, &[REG_ACCEL_XOUT_H], &mut bytes)
            .await
            .map_err(Error::Bus)?;
        Ok(RawSample {
            accel: [
                i16::from_be_bytes([bytes[0], bytes[1]]),
                i16::from_be_bytes([bytes[2], bytes[3]]),
                i16::from_be_bytes([bytes[4], bytes[5]]),
            ],
            temperature: i16::from_be_bytes([bytes[6], bytes[7]]),
            gyro: [
                i16::from_be_bytes([bytes[8], bytes[9]]),
                i16::from_be_bytes([bytes[10], bytes[11]]),
                i16::from_be_bytes([bytes[12], bytes[13]]),
            ],
        })
    }

    /// Read one burst and convert it into SI physical units.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bus`] when the I2C transaction fails.
    pub async fn read_sample(&mut self) -> Result<Sample, Error<I2C::Error>> {
        let raw = self.read_raw().await?;
        let accel_scale = STANDARD_GRAVITY_M_S2 / self.config.accel_range.lsb_per_g();
        let gyro_scale =
            core::f32::consts::PI / (180.0 * self.config.gyro_range.lsb_per_degree_s());
        Ok(Sample {
            accel_m_s2: [
                f32::from(raw.accel[0]) * accel_scale,
                f32::from(raw.accel[1]) * accel_scale,
                f32::from(raw.accel[2]) * accel_scale,
            ],
            gyro_rad_s: [
                f32::from(raw.gyro[0]) * gyro_scale,
                f32::from(raw.gyro[1]) * gyro_scale,
                f32::from(raw.gyro[2]) * gyro_scale,
            ],
            temperature_c: f32::from(raw.temperature) / 340.0 + 36.53,
        })
    }

    async fn write_register(&mut self, register: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.address as u8, &[register, value])
            .await
            .map_err(Error::Bus)
    }
}

fn sample_rate_divider<E>(config: Config) -> Result<u8, Error<E>> {
    let rate = u32::from(config.sample_rate_hz);
    if rate == 0 || rate > 1_000 || 1_000 % rate != 0 {
        return Err(Error::InvalidConfig);
    }
    u8::try_from(1_000 / rate - 1).map_err(|_| Error::InvalidConfig)
}

#[cfg(test)]
mod tests;

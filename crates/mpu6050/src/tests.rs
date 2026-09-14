extern crate std;

use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
use futures::executor::block_on;
use std::collections::VecDeque;
use std::{vec, vec::Vec};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FakeError;

impl embedded_hal::i2c::Error for FakeError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[derive(Debug)]
enum Expectation {
    Write(u8, Vec<u8>),
    WriteRead(u8, Vec<u8>, Vec<u8>),
    FailWriteRead(u8, Vec<u8>),
}

struct FakeI2c {
    expectations: VecDeque<Expectation>,
}

impl FakeI2c {
    fn new(expectations: impl IntoIterator<Item = Expectation>) -> Self {
        Self {
            expectations: expectations.into_iter().collect(),
        }
    }

    fn assert_done(&self) {
        assert!(self.expectations.is_empty(), "unused I2C expectations");
    }
}

impl ErrorType for FakeI2c {
    type Error = FakeError;
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "embedded-hal's async I2C trait requires these test-double methods to be async"
)]
impl I2c for FakeI2c {
    async fn read(&mut self, _address: u8, _read: &mut [u8]) -> Result<(), Self::Error> {
        panic!("unexpected read")
    }

    async fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Self::Error> {
        let Expectation::Write(expected_address, expected) = self
            .expectations
            .pop_front()
            .expect("missing write expectation")
        else {
            panic!("expected a different I2C operation")
        };
        assert_eq!(address, expected_address);
        assert_eq!(write, expected);
        Ok(())
    }

    async fn write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        match self
            .expectations
            .pop_front()
            .expect("missing write_read expectation")
        {
            Expectation::WriteRead(expected_address, expected_write, response) => {
                assert_eq!(address, expected_address);
                assert_eq!(write, expected_write);
                assert_eq!(read.len(), response.len());
                read.copy_from_slice(&response);
                Ok(())
            }
            Expectation::FailWriteRead(expected_address, expected_write) => {
                assert_eq!(address, expected_address);
                assert_eq!(write, expected_write);
                Err(FakeError)
            }
            Expectation::Write(_, _) => panic!("expected write, got write_read"),
        }
    }

    async fn transaction(
        &mut self,
        _address: u8,
        _operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        panic!("unexpected transaction")
    }
}

#[test]
fn initializes_expected_registers() {
    let expectations = [
        Expectation::WriteRead(0x68, vec![REG_WHO_AM_I], vec![WHO_AM_I_MPU6050]),
        Expectation::Write(0x68, vec![REG_PWR_MGMT_1, 0x01]),
        Expectation::Write(0x68, vec![REG_CONFIG, 0x03]),
        Expectation::Write(0x68, vec![REG_SMPLRT_DIV, 0x04]),
        Expectation::Write(0x68, vec![REG_GYRO_CONFIG, 0x08]),
        Expectation::Write(0x68, vec![REG_ACCEL_CONFIG, 0x08]),
    ];
    let i2c = FakeI2c::new(expectations);
    let mut sensor = Mpu6050::new(i2c, Address::Primary, Config::default());
    block_on(sensor.init()).expect("initialization succeeds");
    sensor.release().assert_done();
}

#[test]
fn initializes_known_compatible_clone() {
    let expectations = [
        Expectation::WriteRead(0x68, vec![REG_WHO_AM_I], vec![WHO_AM_I_COMPATIBLE_CLONE]),
        Expectation::Write(0x68, vec![REG_PWR_MGMT_1, 0x01]),
        Expectation::Write(0x68, vec![REG_CONFIG, 0x03]),
        Expectation::Write(0x68, vec![REG_SMPLRT_DIV, 0x04]),
        Expectation::Write(0x68, vec![REG_GYRO_CONFIG, 0x08]),
        Expectation::Write(0x68, vec![REG_ACCEL_CONFIG, 0x08]),
    ];
    let i2c = FakeI2c::new(expectations);
    let mut sensor = Mpu6050::new(i2c, Address::Primary, Config::default());
    block_on(sensor.init()).expect("compatible clone initialization succeeds");
    sensor.release().assert_done();
}

#[test]
fn supports_secondary_address() {
    let i2c = FakeI2c::new([Expectation::WriteRead(
        0x69,
        vec![REG_WHO_AM_I],
        vec![WHO_AM_I_MPU6050],
    )]);
    let mut sensor = Mpu6050::new(i2c, Address::Secondary, Config::default());
    assert_eq!(block_on(sensor.who_am_i()), Ok(WHO_AM_I_MPU6050));
    sensor.release().assert_done();
}

#[test]
fn rejects_unexpected_identity() {
    let i2c = FakeI2c::new([Expectation::WriteRead(0x68, vec![REG_WHO_AM_I], vec![0x00])]);
    let mut sensor = Mpu6050::new(i2c, Address::Primary, Config::default());
    assert_eq!(block_on(sensor.init()), Err(Error::InvalidWhoAmI(0)));
}

#[test]
fn propagates_bus_errors() {
    let i2c = FakeI2c::new([Expectation::FailWriteRead(0x68, vec![REG_WHO_AM_I])]);
    let mut sensor = Mpu6050::new(i2c, Address::Primary, Config::default());
    assert_eq!(block_on(sensor.who_am_i()), Err(Error::Bus(FakeError)));
}

#[test]
fn decodes_and_scales_burst_sample() {
    let values = [8_192_i16, -8_192, 4_096, 340, 65, -131, 0];
    let bytes: Vec<u8> = values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
    let i2c = FakeI2c::new([Expectation::WriteRead(0x68, vec![REG_ACCEL_XOUT_H], bytes)]);
    let mut sensor = Mpu6050::new(i2c, Address::Primary, Config::default());
    let sample = block_on(sensor.read_sample()).expect("sample succeeds");
    assert!((sample.accel_m_s2[0] - STANDARD_GRAVITY_M_S2).abs() < 1.0e-5);
    assert!((sample.accel_m_s2[1] + STANDARD_GRAVITY_M_S2).abs() < 1.0e-5);
    assert!((sample.accel_m_s2[2] - STANDARD_GRAVITY_M_S2 / 2.0).abs() < 1.0e-5);
    assert!((sample.gyro_rad_s[0] - (65.0_f32 / 65.5).to_radians()).abs() < 1.0e-6);
    assert!((sample.gyro_rad_s[1] - (-131.0_f32 / 65.5).to_radians()).abs() < 1.0e-6);
    assert!((sample.temperature_c - 37.53).abs() < 1.0e-5);
}

#[test]
fn invalid_sample_rate_is_rejected_before_io() {
    let i2c = FakeI2c::new(core::iter::empty::<Expectation>());
    let config = Config {
        sample_rate_hz: 333,
        ..Config::default()
    };
    let mut sensor = Mpu6050::new(i2c, Address::Primary, config);
    assert_eq!(block_on(sensor.init()), Err(Error::InvalidConfig));
}

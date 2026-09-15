extern crate std;

use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
use futures::executor::block_on;
use std::collections::VecDeque;
use std::{vec, vec::Vec};

use super::*;

const CALIBRATION_BYTES: [u8; 21] = [
    0x0A, 0x6B, 0xB4, 0x49, 0xF6, 0x0C, 0xFF, 0x4A, 0xF3, 0x23, 0x00, 0x17, 0x65, 0xF5, 0x7A, 0xF3,
    0xF6, 0xD6, 0x3F, 0x1D, 0xC4,
];
const RAW_BYTES: [u8; 6] = [0x70, 0xC2, 0x7D, 0x98, 0x1A, 0x7F];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Default)]
struct FakeDelay {
    delays_ns: Vec<u32>,
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "embedded-hal's async delay trait requires this test-double method to be async"
)]
impl DelayNs for FakeDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.delays_ns.push(ns);
    }
}

#[test]
fn decodes_status_and_error_flags() {
    assert_eq!(
        Status::from_register(0x70),
        Status {
            command_ready: true,
            pressure_ready: true,
            temperature_ready: true,
        }
    );
    assert_eq!(
        SensorErrors::from_register(0x07),
        SensorErrors {
            fatal: true,
            command: true,
            configuration: true,
        }
    );
}

#[test]
fn decodes_raw_measurement_boundaries() {
    for (bytes, expected) in [
        ([0x00, 0x00, 0x00], 0),
        ([0xFF, 0xFF, 0xFF], 0xFF_FFFF),
        ([0x00, 0x00, 0x80], 0x80_0000),
    ] {
        let sample =
            RawSample::from_bytes([bytes[0], bytes[1], bytes[2], bytes[0], bytes[1], bytes[2]]);
        assert_eq!(sample.pressure, expected);
        assert_eq!(sample.temperature, expected);
    }
}

#[test]
fn compensates_bosch_reference_vector() {
    let calibration = Calibration::from_bytes(CALIBRATION_BYTES);
    let sample = calibration.compensate(RawSample::from_bytes(RAW_BYTES));

    assert!((sample.temperature_celsius - 23.045_441_820_862_38).abs() < 1e-10);
    assert!((sample.pressure_pa - 83_922.747_836_956_33).abs() < 1e-8);
}

#[test]
fn rejects_configuration_that_cannot_fit_the_odr() {
    let config = Config {
        pressure_oversampling: Oversampling::X32,
        temperature_oversampling: Oversampling::X32,
        output_data_rate: OutputDataRate::Hz200,
        iir_filter: IirFilter::Off,
    };
    let i2c = FakeI2c::new([]);
    let mut sensor = Bmp388::new(i2c, Address::Primary);

    assert_eq!(
        block_on(sensor.configure(config)),
        Err(Error::InvalidConfig)
    );
    sensor.release().assert_done();
}

#[test]
fn initializes_resets_loads_calibration_and_configures() {
    let expectations = [
        Expectation::WriteRead(0x76, vec![0x00], vec![0x50]),
        Expectation::WriteRead(0x76, vec![0x03], vec![0x10]),
        Expectation::Write(0x76, vec![0x7E, 0xB6]),
        Expectation::WriteRead(0x76, vec![0x02], vec![0x00]),
        Expectation::WriteRead(0x76, vec![0x03], vec![0x10]),
        Expectation::WriteRead(0x76, vec![0x31], CALIBRATION_BYTES.to_vec()),
        Expectation::Write(0x76, vec![0x1B, 0x03]),
        Expectation::Write(0x76, vec![0x1C, 0x0B]),
        Expectation::Write(0x76, vec![0x1D, 0x03]),
        Expectation::Write(0x76, vec![0x1F, 0x04]),
        Expectation::WriteRead(0x76, vec![0x02], vec![0x00]),
    ];
    let i2c = FakeI2c::new(expectations);
    let mut delay = FakeDelay::default();
    let mut sensor = Bmp388::new(i2c, Address::Primary);

    block_on(sensor.init(&mut delay)).expect("initialization succeeds");

    assert_eq!(delay.delays_ns, vec![2_000_000, 2_000_000]);
    sensor.release().assert_done();
}

#[test]
fn performs_a_fresh_forced_measurement() {
    let expectations = [
        Expectation::Write(0x77, vec![0x1B, 0x13]),
        Expectation::WriteRead(0x77, vec![0x03], vec![0x60]),
        Expectation::WriteRead(0x77, vec![0x04], RAW_BYTES.to_vec()),
    ];
    let i2c = FakeI2c::new(expectations);
    let mut delay = FakeDelay::default();
    let mut sensor = Bmp388::new(i2c, Address::Secondary);
    sensor.calibration = Some(Calibration::from_bytes(CALIBRATION_BYTES));

    let sample = block_on(sensor.measure_forced(&mut delay)).expect("measurement succeeds");

    assert!((sample.temperature_celsius - 23.045_441_820_862_38).abs() < 1e-10);
    assert!((sample.pressure_pa - 83_922.747_836_956_33).abs() < 1e-8);
    assert!(delay.delays_ns.is_empty());
    sensor.release().assert_done();
}

#[test]
fn reports_sensor_fault_after_reset() {
    let expectations = [
        Expectation::WriteRead(0x76, vec![0x03], vec![0x10]),
        Expectation::Write(0x76, vec![0x7E, 0xB6]),
        Expectation::WriteRead(0x76, vec![0x02], vec![0x02]),
    ];
    let i2c = FakeI2c::new(expectations);
    let mut delay = FakeDelay::default();
    let mut sensor = Bmp388::new(i2c, Address::Primary);

    assert_eq!(
        block_on(sensor.soft_reset(&mut delay)),
        Err(Error::SensorFault(SensorErrors {
            fatal: false,
            command: true,
            configuration: false,
        }))
    );
    sensor.release().assert_done();
}

#[test]
fn preserves_bus_errors() {
    let i2c = FakeI2c::new([Expectation::FailWriteRead(0x76, vec![0x00])]);
    let mut sensor = Bmp388::new(i2c, Address::Primary);

    assert_eq!(block_on(sensor.chip_id()), Err(Error::Bus(FakeError)));
    sensor.release().assert_done();
}

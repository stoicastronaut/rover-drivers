extern crate std;

use super::*;
use core::{
    future::Future,
    task::{Context, Poll},
};
use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
use futures::{executor::block_on, task::noop_waker};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, vec, vec::Vec};

// Independent reference value so conversion tests also check the production constant.
const EXPECTED_STANDARD_GRAVITY_M_S2: f32 = 9.806_65;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FakeError;
impl embedded_hal::i2c::Error for FakeError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Write(u8, Vec<u8>),
    Read(u8, u8, Vec<u8>),
    Delay(u32),
}
#[derive(Clone, Copy, Default)]
enum Outcome {
    #[default]
    Ok,
    Fail,
    Pending,
}
#[derive(Clone)]
struct Step {
    action: Action,
    outcome: Outcome,
}
impl From<Action> for Step {
    fn from(action: Action) -> Self {
        Self {
            action,
            outcome: Outcome::Ok,
        }
    }
}
#[derive(Clone)]
struct Script(Rc<RefCell<VecDeque<Step>>>);
impl Script {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self(Rc::new(RefCell::new(steps.into_iter().collect())))
    }
    fn next(&self) -> Step {
        self.0
            .borrow_mut()
            .pop_front()
            .expect("unexpected I/O or delay")
    }
    fn assert_done(&self) {
        assert!(self.0.borrow().is_empty(), "unconsumed protocol steps");
    }
}
struct FakeI2c(Script);
struct FakeDelay(Script);
impl ErrorType for FakeI2c {
    type Error = FakeError;
}
async fn complete(outcome: Outcome) -> Result<(), FakeError> {
    match outcome {
        Outcome::Ok => Ok(()),
        Outcome::Fail => Err(FakeError),
        Outcome::Pending => core::future::pending().await,
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "async HAL test-double methods must implement the async trait"
)]
impl I2c for FakeI2c {
    async fn read(&mut self, _: u8, _: &mut [u8]) -> Result<(), FakeError> {
        panic!("unexpected read");
    }
    async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), FakeError> {
        let step = self.0.next();
        assert_eq!(step.action, Action::Write(address, bytes.to_vec()));
        complete(step.outcome).await
    }
    async fn write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), FakeError> {
        let step = self.0.next();
        let Action::Read(expected_address, register, response) = step.action else {
            panic!("expected another operation");
        };
        assert_eq!(address, expected_address);
        assert_eq!(write, &[register]);
        assert_eq!(read.len(), response.len());
        // Even failed/pending reads can have partially modified the destination.
        read.copy_from_slice(&response);
        complete(step.outcome).await
    }
    async fn transaction(&mut self, _: u8, _: &mut [Operation<'_>]) -> Result<(), FakeError> {
        panic!("unexpected transaction");
    }
}
impl DelayNs for FakeDelay {
    async fn delay_ns(&mut self, ns: u32) {
        let step = self.0.next();
        assert_eq!(step.action, Action::Delay(ns));
        complete(step.outcome).await.unwrap();
    }
}
fn write(address: u8, register: u8, value: u8) -> Step {
    Action::Write(address, vec![register, value]).into()
}
fn read(address: u8, register: u8, bytes: &[u8]) -> Step {
    Action::Read(address, register, bytes.to_vec()).into()
}
fn delay(ms: u32) -> Step {
    Action::Delay(ms * 1_000_000).into()
}

// Literal datasheet values, independent of production register constants/encoders.
fn initialization(address: u8) -> Vec<Step> {
    vec![
        delay(100),
        write(address, 0x7f, 0),
        read(address, 0, &[0xea]),
        write(address, 6, 0x80),
        delay(100),
        write(address, 0x7f, 0),
        write(address, 6, 1),
        write(address, 7, 0),
        write(address, 5, 0),
        write(address, 3, 0),
        write(address, 0x7f, 0x20),
        write(address, 0, 4),
        write(address, 1, 0x1b),
        write(address, 0x10, 0),
        write(address, 0x11, 4),
        write(address, 0x14, 0x1b),
        write(address, 0x7f, 0),
        write(address, 0x0f, 2),
        write(0x0c, 0x32, 1),
        delay(1),
        read(0x0c, 1, &[9]),
        write(0x0c, 0x31, 8),
        delay(100),
    ]
}
fn setup(
    steps: Vec<Step>,
    address: Address,
    config: Config,
) -> (Icm20948<FakeI2c>, FakeDelay, Script) {
    let script = Script::new(steps);
    (
        Icm20948::new(FakeI2c(script.clone()), address, config),
        FakeDelay(script.clone()),
        script,
    )
}
fn cancel(future: impl Future) {
    let mut future = std::boxed::Box::pin(future);
    assert!(matches!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(&noop_waker())),
        Poll::Pending
    ));
}
fn burst() -> Vec<u8> {
    [8192_i16, -8192, 4096, 655, -131, i16::MIN, 334]
        .into_iter()
        .flat_map(i16::to_be_bytes)
        .collect()
}
fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn construction_and_uninitialized_reads_do_no_io_and_release_returns_bus() {
    let (mut sensor, _, script) = setup(vec![], Address::Primary, Config::default());
    assert_eq!(block_on(sensor.read_raw()), Err(Error::NotInitialized));
    assert_eq!(block_on(sensor.read_sample()), Err(Error::NotInitialized));
    assert_eq!(
        block_on(sensor.read_magnetic_raw()),
        Err(Error::NotInitialized)
    );
    assert_eq!(
        block_on(sensor.read_magnetic_sample()),
        Err(Error::NotInitialized)
    );
    assert!(Rc::ptr_eq(&sensor.release().0.0, &script.0));
    script.assert_done();
}

#[test]
fn initializes_and_reads_both_host_addresses_with_fixed_magnetometer_address() {
    for address in [Address::Primary, Address::Secondary] {
        let a = address as u8;
        let mut steps = initialization(a);
        steps.extend([
            write(a, 0x7f, 0),
            read(a, 0, &[0xea]),
            write(a, 0x7f, 0),
            read(a, 0x2d, &burst()),
            read(0x0c, 0x10, &[1, 1, 0, 0xff, 0xff, 0x34, 0x12, 0xaa, 0]),
        ]);
        let (mut sensor, mut delay, script) = setup(steps, address, Config::default());
        block_on(sensor.init(&mut delay)).unwrap();
        assert_eq!(block_on(sensor.who_am_i()), Ok(0xea));
        let raw = block_on(sensor.read_raw()).unwrap();
        assert_eq!(
            raw,
            RawSample {
                accel: [8192, -8192, 4096],
                gyro: [655, -131, i16::MIN],
                temperature: 334
            }
        );
        assert_eq!(
            block_on(sensor.read_magnetic_raw()),
            Ok(Some(RawMagneticSample {
                magnetic: [1, -1, 0x1234],
                overrun: false
            }))
        );
        script.assert_done();
    }
}

#[test]
fn rejects_invalid_dividers_before_io() {
    for divider in [4096, u16::MAX] {
        let (mut sensor, mut delay, script) = setup(
            vec![],
            Address::Primary,
            Config {
                accel_sample_rate_divider: divider,
                ..Config::default()
            },
        );
        assert_eq!(block_on(sensor.init(&mut delay)), Err(Error::InvalidConfig));
        script.assert_done();
    }
}

#[test]
fn rejects_unexpected_inertial_and_magnetic_identities() {
    for (index, address, expected_error) in [
        (2, 0x68, Error::InvalidWhoAmI(0x68)),
        (20, 0x0c, Error::InvalidMagnetometerWhoAmI(0x68)),
    ] {
        let mut steps = initialization(0x68);
        steps.truncate(index + 1);
        steps[index] = read(address, u8::from(index != 2), &[0x68]);
        let (mut sensor, mut delay, script) = setup(steps, Address::Primary, Config::default());
        assert_eq!(block_on(sensor.init(&mut delay)), Err(expected_error));
        assert_eq!(block_on(sensor.read_raw()), Err(Error::NotInitialized));
        script.assert_done();
    }
}

#[test]
fn each_initialization_bus_failure_blocks_reads_and_retries_full_sequence() {
    let init = initialization(0x68);
    for index in 0..init.len() {
        if matches!(init[index].action, Action::Delay(_)) {
            continue;
        }
        // Start initialized to ensure a failed reinitialization invalidates old state.
        let mut failed = init[..=index].to_vec();
        failed[index].outcome = Outcome::Fail;
        let steps = init
            .iter()
            .cloned()
            .chain(failed)
            .chain(init.clone())
            .collect();
        let (mut sensor, mut delay, script) = setup(steps, Address::Primary, Config::default());
        block_on(sensor.init(&mut delay)).unwrap();
        assert_eq!(
            block_on(sensor.init(&mut delay)),
            Err(Error::Bus(FakeError))
        );
        assert_eq!(block_on(sensor.read_raw()), Err(Error::NotInitialized));
        assert_eq!(
            block_on(sensor.read_magnetic_raw()),
            Err(Error::NotInitialized)
        );
        block_on(sensor.init(&mut delay)).unwrap();
        script.assert_done();
    }
}

#[test]
fn cancellation_at_every_init_await_requires_complete_reinitialization() {
    let init = initialization(0x68);
    for index in 0..init.len() {
        let mut pending = init[..=index].to_vec();
        pending[index].outcome = Outcome::Pending;
        let steps = init
            .iter()
            .cloned()
            .chain(pending)
            .chain(init.clone())
            .collect();
        let (mut sensor, mut delay, script) = setup(steps, Address::Primary, Config::default());
        block_on(sensor.init(&mut delay)).unwrap();
        cancel(sensor.init(&mut delay));
        assert_eq!(block_on(sensor.read_raw()), Err(Error::NotInitialized));
        assert_eq!(
            block_on(sensor.read_magnetic_raw()),
            Err(Error::NotInitialized)
        );
        block_on(sensor.init(&mut delay)).unwrap();
        script.assert_done();
    }
}

#[test]
fn failed_or_cancelled_inertial_reads_retry_bank_selection_and_entire_burst() {
    for outcome in [Outcome::Fail, Outcome::Pending] {
        for index in 0..2 {
            let reads = [write(0x68, 0x7f, 0), read(0x68, 0x2d, &burst())];
            let mut interrupted = reads[..=index].to_vec();
            interrupted[index].outcome = outcome;
            let steps = initialization(0x68)
                .into_iter()
                .chain(interrupted)
                .chain(reads)
                .collect();
            let (mut sensor, mut delay, script) = setup(steps, Address::Primary, Config::default());
            block_on(sensor.init(&mut delay)).unwrap();
            if matches!(outcome, Outcome::Fail) {
                assert_eq!(block_on(sensor.read_raw()), Err(Error::Bus(FakeError)));
            } else {
                cancel(sensor.read_raw());
            }
            assert_eq!(block_on(sensor.read_raw()).unwrap().temperature, 334);
            script.assert_done();
        }
    }
}

#[test]
fn failed_or_cancelled_identity_reads_can_be_retried_without_initialization() {
    for outcome in [Outcome::Fail, Outcome::Pending] {
        for index in 0..2 {
            let reads = [write(0x68, 0x7f, 0), read(0x68, 0, &[0xea])];
            let mut interrupted = reads[..=index].to_vec();
            interrupted[index].outcome = outcome;
            let steps = interrupted.into_iter().chain(reads).collect();
            let (mut sensor, _, script) = setup(steps, Address::Primary, Config::default());
            if matches!(outcome, Outcome::Fail) {
                assert_eq!(block_on(sensor.who_am_i()), Err(Error::Bus(FakeError)));
            } else {
                cancel(sensor.who_am_i());
            }
            assert_eq!(block_on(sensor.who_am_i()), Ok(0xea));
            assert_eq!(block_on(sensor.read_raw()), Err(Error::NotInitialized));
            script.assert_done();
        }
    }
}

#[test]
fn failed_or_cancelled_magnetic_read_retries_through_st2_even_when_not_ready() {
    for outcome in [Outcome::Fail, Outcome::Pending] {
        let mut interrupted = read(0x0c, 0x10, &[1, 1, 0, 2, 0, 3, 0, 0, 0]);
        interrupted.outcome = outcome;
        let steps = initialization(0x68)
            .into_iter()
            .chain([
                interrupted,
                read(0x0c, 0x10, &[0; 9]),
                read(0x0c, 0x10, &[1, 1, 0, 2, 0, 3, 0, 0, 0]),
            ])
            .collect();
        let (mut sensor, mut delay, script) = setup(steps, Address::Primary, Config::default());
        block_on(sensor.init(&mut delay)).unwrap();
        if matches!(outcome, Outcome::Fail) {
            assert_eq!(
                block_on(sensor.read_magnetic_raw()),
                Err(Error::Bus(FakeError))
            );
        } else {
            cancel(sensor.read_magnetic_raw());
        }
        assert_eq!(block_on(sensor.read_magnetic_raw()), Ok(None));
        assert_eq!(
            block_on(sensor.read_magnetic_raw())
                .unwrap()
                .unwrap()
                .magnetic,
            [1, 2, 3]
        );
        script.assert_done();
    }
}

#[test]
fn magnetic_status_and_conversion_use_st1_and_st2() {
    let steps = initialization(0x68)
        .into_iter()
        .chain([
            read(0x0c, 0x10, &[0, 0, 0, 0, 0, 0, 0, 0, 8]),
            read(0x0c, 0x10, &[1, 0, 0, 0, 0, 0, 0, 0, 8]),
            read(0x0c, 0x10, &[3, 100, 0, 0x9c, 0xff, 0, 0x80, 8, 0x70]),
        ])
        .collect();
    let (mut sensor, mut delay, script) = setup(steps, Address::Primary, Config::default());
    block_on(sensor.init(&mut delay)).unwrap();
    assert_eq!(block_on(sensor.read_magnetic_sample()), Ok(None));
    assert_eq!(
        block_on(sensor.read_magnetic_sample()),
        Err(Error::MagneticOverflow)
    );
    let sample = block_on(sensor.read_magnetic_sample()).unwrap().unwrap();
    assert!(sample.overrun);
    assert_close(sample.magnetic_ut[0], 15.0);
    assert_close(sample.magnetic_ut[1], -15.0);
    assert_close(sample.magnetic_ut[2], -4915.2);
    script.assert_done();
}

#[test]
fn all_full_scale_ranges_encode_and_convert_correctly() {
    let ranges = [
        (AccelRange::G2, GyroRange::Dps250, 0x19, 16384.0, 131.0),
        (AccelRange::G4, GyroRange::Dps500, 0x1b, 8192.0, 65.5),
        (AccelRange::G8, GyroRange::Dps1000, 0x1d, 4096.0, 32.8),
        (AccelRange::G16, GyroRange::Dps2000, 0x1f, 2048.0, 16.4),
    ];
    for (accel_range, gyro_range, register, accel_lsb, gyro_lsb) in ranges {
        let mut steps = initialization(0x68);
        steps[12] = write(0x68, 1, register);
        steps[15] = write(0x68, 0x14, register);
        steps.extend([write(0x68, 0x7f, 0), read(0x68, 0x2d, &burst())]);
        let (mut sensor, mut delay, script) = setup(
            steps,
            Address::Primary,
            Config {
                accel_range,
                gyro_range,
                ..Config::default()
            },
        );
        block_on(sensor.init(&mut delay)).unwrap();
        let sample = block_on(sensor.read_sample()).unwrap();
        for (actual, counts) in sample.accel_m_s2.into_iter().zip([8192.0, -8192.0, 4096.0]) {
            assert_close(actual, counts / accel_lsb * EXPECTED_STANDARD_GRAVITY_M_S2);
        }
        for (actual, counts) in sample.gyro_rad_s.into_iter().zip([655.0, -131.0, -32768.0]) {
            assert_close(actual, counts / gyro_lsb * core::f32::consts::PI / 180.0);
        }
        assert_close(sample.temperature_c, 22.000_39);
        script.assert_done();
    }
}

#[test]
fn filter_choices_encode_without_changing_ranges_or_disabling_filters() {
    let accel_filters = [
        (AccelDlpf::Hz246, 0x0b),
        (AccelDlpf::Hz111_4, 0x13),
        (AccelDlpf::Hz50_4, 0x1b),
        (AccelDlpf::Hz23_9, 0x23),
        (AccelDlpf::Hz11_5, 0x2b),
        (AccelDlpf::Hz5_7, 0x33),
        (AccelDlpf::Hz473, 0x3b),
    ];
    let gyro_filters = [
        (GyroDlpf::Hz151_8, 0x0b),
        (GyroDlpf::Hz119_5, 0x13),
        (GyroDlpf::Hz51_2, 0x1b),
        (GyroDlpf::Hz23_9, 0x23),
        (GyroDlpf::Hz11_6, 0x2b),
        (GyroDlpf::Hz5_7, 0x33),
    ];
    for (accel_dlpf, accel_bits) in accel_filters {
        for (gyro_dlpf, gyro_bits) in gyro_filters {
            let mut steps = initialization(0x68);
            steps[12] = write(0x68, 1, gyro_bits);
            steps[15] = write(0x68, 0x14, accel_bits);
            let (mut sensor, mut delay, script) = setup(
                steps,
                Address::Primary,
                Config {
                    accel_dlpf,
                    gyro_dlpf,
                    ..Config::default()
                },
            );
            block_on(sensor.init(&mut delay)).unwrap();
            script.assert_done();
        }
    }
}

#[test]
fn divider_boundaries_encode_high_and_low_bytes() {
    for (accel, gyro, high, low) in [(0, 0, 0, 0), (256, 128, 1, 0), (4095, 255, 15, 255)] {
        let mut steps = initialization(0x68);
        steps[11] = write(0x68, 0, gyro);
        steps[13] = write(0x68, 0x10, high);
        steps[14] = write(0x68, 0x11, low);
        let (mut sensor, mut delay, script) = setup(
            steps,
            Address::Primary,
            Config {
                accel_sample_rate_divider: accel,
                gyro_sample_rate_divider: gyro,
                ..Config::default()
            },
        );
        block_on(sensor.init(&mut delay)).unwrap();
        script.assert_done();
    }
}

#[test]
fn magnetometer_modes_and_disabled_bypass_match_configuration() {
    for (magnetometer, mode) in [
        (MagnetometerMode::Disabled, 0),
        (MagnetometerMode::Hz10, 2),
        (MagnetometerMode::Hz20, 4),
        (MagnetometerMode::Hz50, 6),
        (MagnetometerMode::Hz100, 8),
    ] {
        let mut steps = initialization(0x68);
        if mode == 0 {
            steps.truncate(18);
            steps[17] = write(0x68, 0x0f, 0);
            steps.push(delay(100));
            steps.extend([write(0x68, 0x7f, 0), read(0x68, 0x2d, &burst())]);
        } else {
            steps[21] = write(0x0c, 0x31, mode);
        }
        let (mut sensor, mut delay, script) = setup(
            steps,
            Address::Primary,
            Config {
                magnetometer,
                ..Config::default()
            },
        );
        block_on(sensor.init(&mut delay)).unwrap();
        if mode == 0 {
            assert_eq!(
                block_on(sensor.read_magnetic_raw()),
                Err(Error::MagnetometerDisabled)
            );
            assert_eq!(
                block_on(sensor.read_magnetic_sample()),
                Err(Error::MagnetometerDisabled)
            );
            block_on(sensor.read_raw()).unwrap();
        }
        script.assert_done();
    }
}

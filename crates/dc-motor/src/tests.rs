use super::*;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
};
use futures::{executor::block_on, task::noop_waker};
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Write(char, u16),
    Wait(u32),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PwmError;
impl embedded_hal::pwm::Error for PwmError {
    fn kind(&self) -> embedded_hal::pwm::ErrorKind {
        embedded_hal::pwm::ErrorKind::Other
    }
}
struct Pwm {
    leg: char,
    maximum: u16,
    always_fail: bool,
    events: Rc<RefCell<Vec<Event>>>,
    fail: Rc<RefCell<Option<usize>>>,
    writes: Rc<RefCell<usize>>,
}
impl embedded_hal::pwm::ErrorType for Pwm {
    type Error = PwmError;
}
impl SetDutyCycle for Pwm {
    fn max_duty_cycle(&self) -> u16 {
        self.maximum
    }
    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Self::Error> {
        assert!(duty <= self.maximum);
        self.events.borrow_mut().push(Event::Write(self.leg, duty));
        let n = *self.writes.borrow();
        *self.writes.borrow_mut() += 1;
        if self.always_fail || *self.fail.borrow() == Some(n) {
            Err(PwmError)
        } else {
            Ok(())
        }
    }
}
struct Delay {
    events: Rc<RefCell<Vec<Event>>>,
    pending: bool,
}
impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.events.borrow_mut().push(Event::Wait(ns));
        if self.pending {
            core::future::pending::<()>().await;
        }
    }
}
struct Fixture {
    motor: Motor<Pwm, Pwm>,
    delay: Delay,
    events: Rc<RefCell<Vec<Event>>>,
    fail: Rc<RefCell<Option<usize>>>,
    writes: Rc<RefCell<usize>>,
}
impl Fixture {
    fn new(inverted: bool) -> Self {
        let events = Rc::new(RefCell::new(Vec::new()));
        let fail = Rc::new(RefCell::new(None));
        let writes = Rc::new(RefCell::new(0));
        let pwm = |leg, maximum| Pwm {
            leg,
            maximum,
            always_fail: false,
            events: events.clone(),
            fail: fail.clone(),
            writes: writes.clone(),
        };
        Self {
            motor: Motor::new(
                MotorId::new(7).unwrap(),
                pwm('A', 1000),
                pwm('B', u16::MAX),
                MotorConfig::new(inverted, 2000).unwrap(),
            ),
            delay: Delay {
                events: events.clone(),
                pending: false,
            },
            events,
            fail,
            writes,
        }
    }
    fn initialize(&mut self) {
        block_on(self.motor.initialize(&mut self.delay)).unwrap();
        self.events.borrow_mut().clear();
    }
}
#[test]
fn construction_validation_and_lifecycle() {
    assert_eq!(MotorId::new(0), Err(InvalidMotorId));
    assert_eq!(MotorConfig::new(false, 0), Err(InvalidConfig));
    let mut f = Fixture::new(false);
    assert!(f.events.borrow().is_empty());
    assert_eq!(f.motor.id().get(), 7);
    assert_eq!(f.motor.state(), None);
    assert_eq!(f.motor.set_power(Power::FULL), Err(Error::NotInitialized));
    assert_eq!(
        block_on(f.motor.set_direction(MotorState::Forward, &mut f.delay)),
        Err(Error::NotInitialized)
    );
    block_on(f.motor.initialize(&mut f.delay)).unwrap();
    assert_eq!(
        *f.events.borrow(),
        vec![
            Event::Write('A', 0),
            Event::Write('B', 0),
            Event::Wait(2_000_000)
        ]
    );
    assert_eq!(
        f.motor.set_power(Power::FULL),
        Err(Error::DirectionRequired)
    );
    assert_eq!(f.motor.set_power(Power::ZERO), Ok(()));
    let (a, b) = f.motor.release();
    assert_eq!((a.leg, b.leg), ('A', 'B'));
}
#[test]
fn duty_endpoints_and_intermediate_scale_on_each_leg() {
    for (state, leg, maximum) in [
        (MotorState::Forward, 'A', 1000),
        (MotorState::Backward, 'B', 65535),
    ] {
        let mut f = Fixture::new(false);
        f.initialize();
        block_on(f.motor.set_direction(state, &mut f.delay)).unwrap();
        f.events.borrow_mut().clear();
        for power in [Power::ZERO, Power::new(128), Power::FULL] {
            f.motor.set_power(power).unwrap();
        }
        assert_eq!(
            *f.events.borrow(),
            vec![
                Event::Write(leg, 0),
                Event::Write(leg, u16::try_from(u32::from(maximum) * 128 / 255).unwrap()),
                Event::Write(leg, maximum)
            ]
        );
    }
}
#[test]
fn polarity_and_reversal_disable_active_leg_first_and_clear_power() {
    for inverted in [false, true] {
        let mut f = Fixture::new(inverted);
        f.initialize();
        block_on(
            f.motor
                .drive(MotorState::Forward, Power::FULL, &mut f.delay),
        )
        .unwrap();
        f.events.borrow_mut().clear();
        block_on(f.motor.set_direction(MotorState::Backward, &mut f.delay)).unwrap();
        let (first, second) = if inverted { ('B', 'A') } else { ('A', 'B') };
        assert_eq!(
            *f.events.borrow(),
            vec![
                Event::Write(first, 0),
                Event::Write(second, 0),
                Event::Wait(2_000_000)
            ]
        );
        assert_eq!(f.motor.power(), Power::ZERO);
        f.motor.set_power(Power::FULL).unwrap();
        assert_eq!(
            f.events.borrow().last(),
            Some(&Event::Write(
                if inverted { 'A' } else { 'B' },
                if inverted { 1000 } else { 65535 }
            ))
        );
    }
}
#[test]
fn same_direction_updates_without_wait_and_stop_never_restores_power() {
    let mut f = Fixture::new(false);
    f.initialize();
    block_on(
        f.motor
            .drive(MotorState::Forward, Power::FULL, &mut f.delay),
    )
    .unwrap();
    f.events.borrow_mut().clear();
    block_on(
        f.motor
            .drive(MotorState::Forward, Power::new(128), &mut f.delay),
    )
    .unwrap();
    assert_eq!(*f.events.borrow(), vec![Event::Write('A', 501)]);
    block_on(
        f.motor
            .drive(MotorState::Stopped, Power::FULL, &mut f.delay),
    )
    .unwrap();
    assert_eq!(f.motor.state(), Some(MotorState::Stopped));
    assert_eq!(f.motor.power(), Power::ZERO);
    block_on(f.motor.set_direction(MotorState::Forward, &mut f.delay)).unwrap();
    assert_eq!(f.motor.power(), Power::ZERO);
}
#[test]
fn failure_at_each_initialization_and_transition_write_attempts_both_cleanup_legs() {
    for initialization in [false, true] {
        for offset in 0..2 {
            let mut f = Fixture::new(false);
            if !initialization {
                f.initialize();
                block_on(
                    f.motor
                        .drive(MotorState::Backward, Power::FULL, &mut f.delay),
                )
                .unwrap();
                f.events.borrow_mut().clear();
            }
            *f.fail.borrow_mut() = Some(*f.writes.borrow() + offset);
            let result = if initialization {
                block_on(f.motor.initialize(&mut f.delay))
            } else {
                block_on(f.motor.set_direction(MotorState::Forward, &mut f.delay))
            };
            assert!(matches!(result, Err(Error::OutputA(_) | Error::OutputB(_))));
            let events = f.events.borrow();
            assert_eq!(
                &events[events.len() - 2..],
                &[Event::Write('A', 0), Event::Write('B', 0)]
            );
            assert!(!f.motor.is_initialized());
            assert_eq!(f.motor.state(), None);
            assert_eq!(f.motor.set_power(Power::FULL), Err(Error::NotInitialized));
            drop(events);
            f.initialize();
            assert_eq!(f.motor.state(), Some(MotorState::Stopped));
        }
    }
}
#[test]
fn active_pwm_failure_on_both_legs_invalidates_state_and_stop_can_retry() {
    for state in [MotorState::Forward, MotorState::Backward] {
        let mut f = Fixture::new(false);
        f.initialize();
        block_on(f.motor.set_direction(state, &mut f.delay)).unwrap();
        *f.fail.borrow_mut() = Some(*f.writes.borrow());
        let error = f.motor.set_power(Power::FULL);
        assert_eq!(
            error,
            if state == MotorState::Forward {
                Err(Error::OutputA(PwmError))
            } else {
                Err(Error::OutputB(PwmError))
            }
        );
        assert_eq!(f.motor.state(), None);
        block_on(f.motor.stop(&mut f.delay)).unwrap();
        assert!(!f.motor.is_initialized());
    }
}
#[test]
fn cancellation_at_wait_leaves_disabled_and_initialization_requires_retry() {
    for initialization in [true, false] {
        let mut f = Fixture::new(false);
        if !initialization {
            f.initialize();
            block_on(
                f.motor
                    .drive(MotorState::Forward, Power::FULL, &mut f.delay),
            )
            .unwrap();
        }
        f.events.borrow_mut().clear();
        f.delay.pending = true;
        {
            if initialization {
                let mut future = pin!(f.motor.initialize(&mut f.delay));
                assert_eq!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(&noop_waker())),
                    Poll::Pending
                );
            } else {
                let mut future = pin!(f.motor.set_direction(MotorState::Backward, &mut f.delay));
                assert_eq!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(&noop_waker())),
                    Poll::Pending
                );
            }
        }
        assert_eq!(f.motor.state(), Some(MotorState::Stopped));
        assert_eq!(f.motor.power(), Power::ZERO);
        assert_eq!(f.motor.is_initialized(), !initialization);
        assert_eq!(
            *f.events.borrow(),
            vec![
                Event::Write('A', 0),
                Event::Write('B', 0),
                Event::Wait(2_000_000)
            ]
        );
    }
}

#[test]
fn missing_pwm_configuration_is_rejected_after_disabling_outputs() {
    for leg in ['A', 'B'] {
        let mut f = Fixture::new(false);
        if leg == 'A' {
            f.motor.a.maximum = 0;
        } else {
            f.motor.b.maximum = 0;
        }
        assert_eq!(
            block_on(f.motor.initialize(&mut f.delay)),
            Err(Error::InvalidPwmMaximum)
        );
        assert_eq!(
            *f.events.borrow(),
            vec![Event::Write('A', 0), Event::Write('B', 0)]
        );
        assert!(!f.motor.is_initialized());
        assert_eq!(f.motor.state(), None);
    }
}

#[test]
fn persistent_shutdown_errors_still_attempt_both_legs_and_preserve_original_error() {
    let mut f = Fixture::new(false);
    f.initialize();
    block_on(
        f.motor
            .drive(MotorState::Backward, Power::FULL, &mut f.delay),
    )
    .unwrap();
    f.events.borrow_mut().clear();
    f.motor.a.always_fail = true;
    f.motor.b.always_fail = true;
    assert_eq!(
        block_on(f.motor.stop(&mut f.delay)),
        Err(Error::OutputB(PwmError))
    );
    assert_eq!(
        *f.events.borrow(),
        vec![
            Event::Write('B', 0),
            Event::Write('A', 0),
            Event::Write('B', 0)
        ]
    );
    assert_eq!(f.motor.state(), None);
    assert!(!f.motor.is_initialized());
}

#[test]
fn cancelling_stop_never_restores_running_power() {
    let mut f = Fixture::new(false);
    f.initialize();
    block_on(
        f.motor
            .drive(MotorState::Backward, Power::FULL, &mut f.delay),
    )
    .unwrap();
    f.events.borrow_mut().clear();
    f.delay.pending = true;
    {
        let mut future = pin!(f.motor.stop(&mut f.delay));
        assert_eq!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(&noop_waker())),
            Poll::Pending
        );
    }
    assert_eq!(
        *f.events.borrow(),
        vec![
            Event::Write('B', 0),
            Event::Write('A', 0),
            Event::Wait(2_000_000)
        ]
    );
    assert_eq!(
        f.motor.set_power(Power::FULL),
        Err(Error::DirectionRequired)
    );
    assert_eq!(f.motor.power(), Power::ZERO);
}

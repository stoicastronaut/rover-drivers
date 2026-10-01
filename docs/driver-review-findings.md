# Driver review findings

Reviewed on 2026-10-01 against `main` at
`1a2368ecdf59b9d9173fb96cc610ab1ad03ce635`, using Rust 1.98.0 and the existing
lockfile. This is a findings-only review: no driver, dependency, or test changes
are proposed in this PR. Each finding belongs to one driver and has its own ID.

## Scope and interpretation

The review covered all five driver implementations, their public contracts,
host tests, catalog wiring, and the display's relevant upstream transfer code.
It focused on register sequencing, numerical conversion, partial failure,
cancellation, resource ownership, and availability. The bundled BMP388 and
ICM-20948 datasheets were consulted. MPU6050 rate behavior was cross-checked
against the [I2Cdevlib register documentation](https://github.com/jrowberg/i2cdevlib/blob/master/Arduino/MPU6050/MPU6050.cpp)
(`getRate` and `getDLPFMode`); the manufacturer's PDF could not be downloaded
from this environment.

**Defect** means behavior contradicts the intended API or device protocol.
**Hardening** means an observed hazard already has a documented caller
responsibility, or protection is currently outside the API's contract.
**Improvement** means an optional tradeoff, not a demonstrated failure.
Medium severity indicates incorrect measurements, ambiguous device state, or
availability risk under the stated trigger. Low indicates diagnostics,
performance, documentation, or development-quality impact. Severity must be
reassessed in the consuming firmware; no remotely exploitable vulnerability or
critical safety failure was established.

There are **17 individual findings**: BMP388 (7), MPU6050 (3), ICM-20948 (2),
GM009605 (2), and DC motor (3). No finding is a confirmed heap leak.

## BMP388 (`crates/bmp388`)

### Correctness and lifecycle

#### BMP-01 — Failed initial configuration still admits compensated readings

**Medium · Defect · Reproduced with a fake bus.**

**Location:** [`init`](../crates/bmp388/src/lib.rs#L392) and
[`read_latest`](../crates/bmp388/src/lib.rs#L524).

`init` publishes `Some(calibration)` before `configure` succeeds. Fail the
oversampling write at `0x1C` during the first initialization: `init` returns
`Err(Bus(...))`, but `read_latest` still returns `Ok(Measurement)`. Normal and
forced-mode entry points also use calibration presence as their initialization
guard. This contradicts the documented requirement for successful initialization
and can expose data with an incomplete configuration.

**Suggested follow-up:** Track completed initialization separately, invalidate
it when initialization starts, and publish readiness only after configuration
succeeds. Add failure/cancellation tests at every initialization await, including
after calibration is read. Keep the explicitly unguarded raw-data API separate.

#### BMP-02 — Partial reconfiguration leaves an obsolete conversion-time budget

**Medium · Hardening · Reproduced with a fake bus.**

**Location:** [`configure`](../crates/bmp388/src/lib.rs#L480) and
[`forced_poll_attempts`](../crates/bmp388/src/lib.rs#L691).

After successful initialization, request X32 pressure oversampling and 6.25 Hz,
then fail the ODR write at `0x1D`. The preceding oversampling write succeeds,
but `self.config` remains the old configuration and measurements remain admitted.
With the default temperature oversampling, the old conversion estimate is
20.939 ms while the newly written oversampling needs 68.939 ms. Continuing after
the error can therefore cause premature measurement timeouts, and `config()`
no longer describes the hardware.

The README already tells callers to reinitialize after errors; this finding is
the absence of enforcement, not a claim that the documented recovery fails.

**Suggested follow-up:** Invalidate measurement readiness before configuration
writes and require recovery after any failure/cancellation. Test a successful
oversampling write followed by failed ODR/filter writes. Do not merely update
`self.config` early, which would hide a different partial-write state.

#### BMP-03 — Cancelling a reset write can retain stale calibration/readiness

**Medium · Hardening · Reproduced with a pending fake write.**

**Location:** [`soft_reset`](../crates/bmp388/src/lib.rs#L458).

Calibration and `normal_mode` are invalidated only after awaiting the reset
write. A HAL can transmit the command and still be pending, or report an error
after the device accepted it. Poll a reset until the `0x7E/0xB6` write is pending,
then drop the future: `read_latest` remains permitted using the old calibration.
The fake proves retained software state; actual reset acceptance depends on the
HAL and device. Calibration coefficients may remain numerically identical, but
the device's configuration and mode have been reset.

The README's reinitialize-after-cancellation rule mitigates this if followed.

**Suggested follow-up:** Invalidate cached readiness before issuing a potentially
effective reset write. Test both error-after-transmission and cancellation, and
require full initialization before compensated reads resume.

#### BMP-04 — Normal mode can start in hardware while software reports sleep

**Medium · Hardening · Reproduced with a fake bus.**

**Location:** [`start_normal_mode`](../crates/bmp388/src/lib.rs#L568).

The driver writes `0x33` to `PWR_CTRL`, then awaits an error-register read before
setting `normal_mode = true`. If that read fails or is cancelled, the sensor can
already be converting, while `read_normal_measurement` returns `InvalidState`.
A subsequent `configure` is also admitted because its mode guard is false.
The repro succeeds at the mode write and fails the `ERR_REG` read.

This is another case where the documented full reinitialization recovery is
necessary but not enforced by the API.

**Suggested follow-up:** Represent a pending/unknown mode explicitly, or invalidate
readiness on a failed transition. Block operations requiring a known sleep state
until recovery; test cancellation after the mode write independently of failure
during the write.

#### BMP-05 — A forced measurement can consume an older unread sample

**Medium · Defect · Register sequence reproduced; device behavior supported by the datasheet.**

**Location:** [`measure_forced`](../crates/bmp388/src/lib.rs#L539) and
[`wait_for_data_ready`](../crates/bmp388/src/lib.rs#L668).

If a previous normal/forced conversion left unread data, both ready flags can
already be set. `measure_forced` writes the trigger and immediately accepts those
flags, then returns the previous registers as its promised fresh conversion.
The fake-bus repro returns status `0x70`; the driver performs only the status
read and data read, with no wait or pre-trigger drain.

The [BMP388 datasheet](../data_sheets/bst-bmp388-ds001.pdf), section 4.3.3,
specifies that `drdy_press` and `drdy_temp` reset when the corresponding data
registers are read. An old ready flag is not evidence of completion of the new
trigger. A cancelled prior measurement and a normal-to-forced transition are
concrete ways to encounter this condition.

**Suggested follow-up:** First establish a completed sleep transition, drain
previous data/readiness, then trigger and await the new conversion. Add a model
test with distinguishable old/new samples and a delayed conversion completion.
Verify the resulting sequence on hardware.

#### BMP-06 — The fixed normal-to-forced delay is shorter than a legal conversion

**Medium · Defect · Timing sequence reproduced and checked against the datasheet.**

**Location:** [`measure_forced`](../crates/bmp388/src/lib.rs#L550) and
[`stop_normal_mode`](../crates/bmp388/src/lib.rs#L584).

Stopping normal mode only writes the sleep request. The forced path waits a
fixed 5 ms and sends another mode request. At X32 pressure and X32 temperature,
the driver's own conversion formula gives **128.939 ms**, a valid setting at
6.25 Hz. The repro confirms the intervening delay remains exactly 5 ms.

The [BMP388 datasheet](../data_sheets/bst-bmp388-ds001.pdf), section 3.3.4,
says mode switching waits for the ongoing measurement to finish and further
mode commands are ignored until the pending transition completes. Thus the
forced request can be ignored if it arrives during an active normal conversion.
This is distinct from BMP-05: even after fixing stale data detection, a trigger
sent too early is still ineffective. `stop_normal_mode` also returns before
sleep is guaranteed, so immediate follow-on configuration needs review.

**Suggested follow-up:** Wait for a verified sleep transition using the device's
documented timing/state semantics and a bounded timeout before further mode or
configuration commands. Test the longest supported conversion with stop called
immediately after conversion begins.

### Data integrity

#### BMP-07 — Silent saturation hides invalid compensation results

**Medium · Improvement · Reproduced with zero calibration bytes.**

**Location:** [`Calibration::compensate`](../crates/bmp388/src/lib.rs#L315).

Compensation clamps pressure to 30,000–125,000 Pa and temperature to −40–85 °C,
without exposing whether saturation occurred. With a valid chip ID, no reported
sensor error, all-zero calibration, and a successful data read, the repro returns
`Ok` with exactly 30,000 Pa and 0 °C. A bad calibration/data transfer can therefore
look like a legitimate boundary reading to firmware that checks only range.

Clamping can be an intentional numeric policy; this is an observability and
fault-detection improvement, not a claim that finite inputs cause memory
corruption or that all endpoint values are invalid.

**Suggested follow-up:** Expose an out-of-range/saturation flag or unsaturated
compensated values, and define validation for clearly invalid calibration
blocks. Add invalid-calibration and boundary vectors without rejecting genuine
physical endpoint measurements solely because they equal a limit.

**Memory/security assessment:** No heap allocation, ownership cycle, or unsafe
block was found in this driver's production code. Calibration and bursts use
fixed storage. Poll counts are bounded, but an individual HAL I2C await can
still hang unless firmware/HAL provides a deadline. The findings above concern
data integrity and state, not a demonstrated memory-safety exploit.

## MPU6050 (`crates/mpu6050`)

### Correctness

#### MPU-01 — DLPF setting zero produces an eightfold sample-rate error

**Medium · Defect · Divider write reproduced and hardware rate rule cross-checked.**

**Location:** [`sample_rate_divider`](../crates/mpu6050/src/lib.rs#L270) and
[`Dlpf::Hz260`](../crates/mpu6050/src/lib.rs#L99).

The divider calculation always uses a 1,000 Hz source. DLPF configuration zero
(`Hz260`) uses an 8,000 Hz gyroscope output rate. Requesting 200 Hz with this
filter writes `SMPLRT_DIV = 4`, yielding **1,600 Hz**, rather than divider 39 for
200 Hz. This changes timing and aliasing behavior and can repeat accelerometer
samples because its source rate remains 1 kHz.

**Suggested follow-up:** Select the divider base from the DLPF mode and validate
the resulting divider width and exact-rate policy. Test every DLPF setting,
including the 200 Hz example and low/high supported rate boundaries.

### Lifecycle hardening

#### MPU-02 — Converted reads cannot enforce completed initialization

**Medium · Hardening · Reproduced with a fake bus.**

**Location:** [`Mpu6050`](../crates/mpu6050/src/lib.rs#L147),
[`init`](../crates/mpu6050/src/lib.rs#L180), and
[`read_sample`](../crates/mpu6050/src/lib.rs#L242).

There is no readiness state. Reads before initialization, or after a partially
failed/cancelled initialization, are converted using the requested ranges even
if the device still uses another range. The repro performs no initialization,
supplies raw acceleration 16,384 (1 g at ±2 g), and obtains about 19.6133 m/s²
(2 g) under the driver's default ±4 g scaling.

This limitation is explicitly documented in the README, so it is an API
hardening opportunity rather than an undisclosed promise of guarded reads.

**Suggested follow-up:** Gate physical-unit reads on successful initialization,
invalidate before the first configuration await, and test failures/cancellation
at each write. An explicitly advanced raw-read path can remain unguarded.

### Documentation

#### MPU-03 — The advertised sample-rate range includes impossible dividers

**Low · Defect · Reproduced through the public API.**

**Location:** [README configuration section](../crates/mpu6050/README.md#configuration-and-readings)
and [`sample_rate_divider`](../crates/mpu6050/src/lib.rs#L270).

The README says exact divisors of 1,000 Hz from 1 through 1,000 Hz are valid.
However, 1 Hz requires divider 999 and 2 Hz requires 499; neither fits the
8-bit register. The repro confirms 1 Hz returns `InvalidConfig` before I/O.
At the current 1 kHz base, the accepted exact integer rates are 4, 5, 8, 10, 20,
25, 40, 50, 100, 125, 200, 250, 500, and 1,000 Hz.

**Suggested follow-up:** Document the divider-width restriction and either list
accepted rates or expose divider/actual-rate helpers. Update the wording again
when MPU-01 makes the base frequency filter-dependent.

**Memory/security assessment:** No production heap allocation or unsafe block
was found. The 14-byte read buffer and fixed indexing do not expose an observed
out-of-bounds path. Startup settling, recovery from prior external register
configuration, and the accepted `0x70` clone's electrical behavior still need
hardware validation; this review does not certify clone compatibility.

## ICM-20948 (`crates/icm20948`)

### Availability

#### ICM-01 — A stalled HAL operation can hold the initialization future indefinitely

**Medium · Hardening · Source inspection; dependent on the consuming HAL.**

**Location:** [`init_inertial`](../crates/icm20948/src/lib.rs#L288) and
[`init_magnetometer`](../crates/icm20948/src/lib.rs#L341).

The driver has startup delays and strong cancellation-state handling, but no
deadline around individual bus awaits. A HAL that remains pending after a stuck
bus or excessive clock stretching leaves initialization pending while holding
the mutable driver borrow. This can block sensor acquisition, and a shared-bus
adapter may also hold its bus lock. A faulty peripheral is a sufficient trigger;
no attacker-controlled network path is established.

An async I2C trait does not itself promise deadlines, so this is an integration
requirement, not a claim the driver violates the trait. Other I2C drivers using
the same unbounded HAL have the same general exposure.

**Suggested follow-up:** Document and test a firmware-level deadline and HAL bus
recovery policy. Use the existing cancellation tests as a starting point, but
also exercise a permanently pending transaction. A portable driver need not
select an executor; require a timeout-capable adapter or caller wrapper, and
explain that dropping a future does not by itself repair hardware or a bus lock.

### Measurement usability

#### ICM-02 — Inertial reads provide no freshness indicator

**Low · Improvement · Source and API contract inspection.**

**Location:** [`read_raw`](../crates/icm20948/src/lib.rs#L397) and
[`read_sample`](../crates/icm20948/src/lib.rs#L416).

The API returns the latest inertial registers without checking data-ready or
providing a sequence/timestamp. Faster polling, a low configured ODR, or
initial reading before the first sample can produce repeated/stale values that
look like ordinary successful samples. A fusion/control loop cannot infer
freshness from `Ok(Sample)` alone. The README already describes repeated samples
and separately timed magnetic readings; this is not a protocol bug.

**Suggested follow-up:** Add an optional data-ready API or document a concrete
interrupt/timestamp acquisition pattern. Test a low-rate configuration and
multiple host polls between conversions. Preserve the current latest-value API
for users who intentionally want it.

**Memory/security assessment:** No concrete heap leak, unsafe access, or register
decoding defect was found. The existing tests cover errors/cancellation across
initialization and reads. Fixed-address magnetometer bypass and native-axis
differences are already documented constraints, not newly discovered bugs.
Neither HAL memory safety nor arbitrary external reconfiguration is certified.

## GM009605 (`crates/gm009605`)

### Initialization and display integrity

#### OLED-01 — The panel is enabled before the initial framebuffer is sent

**Low · Hardening · Command order reproduced with the locked upstream dependency.**

**Location:** [`init`](../crates/gm009605/src/lib.rs#L76) and
[`ssd1306` 0.10.0 initialization](https://docs.rs/ssd1306/0.10.0/src/ssd1306/lib.rs.html).

`init_with_addr_mode` ends by sending display-on (`0xAF`); this wrapper calls
`send_frame` only afterward. The probe confirms `0xAF` precedes the first data
transfer. On initialization with unknown panel RAM, or reinitialization while
the local image differs from panel RAM, old/undefined pixels can be visible
before the frame finishes. A transfer failure can leave the enabled panel with
an incomplete image. Actual visual effects require hardware confirmation.

**Suggested follow-up:** Support initialization that keeps the panel off until
the first complete frame succeeds. Check failure/cancellation before and during
that frame, and assert display-on occurs afterward. Simply sending display-off
after the upstream initializer still leaves an earlier display-on interval.

### Performance

#### OLED-02 — A single changed pixel retransmits the entire 1 KiB frame

**Low · Improvement · Reproduced with a fake bus.**

**Location:** [`dirty`](../crates/gm009605/src/lib.rs#L48),
[`flush`](../crates/gm009605/src/lib.rs#L100), and
[`send_frame`](../crates/gm009605/src/lib.rs#L117).

Changing one pixel after initialization causes 66 I2C writes: two window
commands and 64 data chunks containing 1,024 framebuffer bytes. At 400 kHz this
costs approximately 26 ms of aggregate wire time, excluding scheduling. Frequent
small status updates can consume shared-bus bandwidth needed by sensors. It
does not necessarily hold a shared lock for the entire frame; that depends on
the adapter's transaction granularity. The README already documents full-frame
updates, so this is an explicit performance tradeoff, not a leak or a broken
dirty flag.

**Suggested follow-up:** If measurements show contention, track dirty pages or
rectangles and transfer only affected spans. Keep the existing full-frame retry
guarantees, or define equally reliable partial-update recovery. Benchmark bus
time and added RAM before increasing complexity.

**Memory/security assessment:** The framebuffer is a fixed 1,024-byte member,
not a growing allocation. Pixel clipping precedes indexing, including negative
and extreme coordinates, and no leak or out-of-bounds write was demonstrated.
Account for the framebuffer and async state in the target task's stack/static
budget; no target-specific stack exhaustion was measured. Bus-error detail is
lost through the upstream display interface, as already documented.

## DC motor (`crates/dc-motor`)

### Safety and lifecycle

#### MOTOR-01 — Handle release does not require or perform a shutdown

**Medium · Hardening · Source inspection; documented behavior.**

**Location:** [`release`](../crates/dc-motor/src/lib.rs#L124) and
[lifecycle documentation](../crates/dc-motor/README.md#lifecycle-and-transitions).

Calling `release` on a running motor returns both PWM handles without a zero-duty
write; the handles can remain live and driving. Dropping the wrapper also has
no driver-provided stop operation, although the concrete HAL handle's destructor
may disable hardware. This is not a Rust memory leak. A task teardown or error
path that omits `stop` can leave an actuator energized, depending on the backend.
The README explicitly requires callers to stop first and treats hardware power
disconnect as an independent safety boundary.

**Suggested follow-up:** Consider an explicit async `stop_and_release` operation
or a checked release requiring a stopped state. Define ownership/recovery on
shutdown error and cancellation. Document a firmware watchdog or independent
hardware disable for abandoned tasks; do not promise an async destructor or
treat successful software shutdown as a complete physical safety guarantee.

### Development quality

#### MOTOR-02 — Missing public error documentation fails the required Clippy gate

**Low · Defect · Reproduced by the required workspace command.**

**Location:** [`MotorId::new`](../crates/dc-motor/src/lib.rs#L20),
[`MotorConfig::new`](../crates/dc-motor/src/lib.rs#L64), and
[`Motor::initialize`](../crates/dc-motor/src/lib.rs#L173).

These three fallible public methods lack `# Errors` sections. With the workspace's
pedantic policy, `cargo clippy --workspace --all-targets -- -D warnings` fails
with three `missing_errors_doc` errors. This blocks the documented quality gate
even though host tests pass.

**Suggested follow-up:** Document each method's actual failure conditions and
initialization/cleanup effects. Keep the lint enabled and rerun Clippy.

#### MOTOR-03 — An extra blank line fails the required formatting gate

**Low · Defect · Reproduced by rustfmt 1.98.0.**

**Location:** [before `Motor`](../crates/dc-motor/src/lib.rs#L83).

`cargo fmt --all -- --check` reports the extra blank line between `MotorConfig`'s
implementation and `Motor`. The formatting check therefore fails on the reviewed
baseline independently of MOTOR-02.

**Suggested follow-up:** Remove that formatting difference in a separate fix and
rerun the formatting check.

**Memory/security assessment:** No allocation, ownership cycle, or unsafe block
was found. The `expect` in duty scaling is not an observed panic bug: the maximum
product is `255 * 65535 = 16711425`, which fits `u32`, and division by 255 fits
`u16`. Existing tests exercise failed cleanup and cancellation. Commanded state
is not proof of wheel motion or physical disablement, as the documentation notes.

## Catalog (`crates/rover-drivers`)

The catalog only conditionally re-exports the five driver crates. The
`all-drivers` feature check passed. No separate runtime, allocation, or security
finding was identified in this crate.

## Validation and limits

Validation was rerun on the unchanged driver source while preparing this
documentation-only branch:

| Check | Result |
| --- | --- |
| `cargo +1.98.0 test --workspace --locked` | 46 unit tests and 1 doctest passed; 8 doctests ignored by the repository |
| `cargo +1.98.0 check -p rover-drivers --features all-drivers --locked` | Passed |
| `cargo +1.98.0 clippy --workspace --all-targets --locked -- -D warnings` | Failed on the three existing MOTOR-02 documentation errors |
| `cargo +1.98.0 fmt --all -- --check` | Failed on the existing MOTOR-03 blank line |
| Isolated public-API fake-bus probes | 12 observations reproduced; no driver source modifications |

The isolated probes covered BMP-01 through BMP-07, MPU-01 through MPU-03, and
OLED-01/OLED-02. They used a separate temporary Cargo package outside the checkout
with path dependencies on these drivers, immediate fake delays, scripted I2C
responses/failures, and a pending reset-write future polled once then dropped.
Each relevant entry above specifies its trigger and observation so it can be
turned into a regression test. The probes assert the current problematic
behavior, not the desired fixed behavior; their passing result does not mean
the issues are fixed. The temporary harness is not part of this PR.

No physical sensors, PWM hardware, or panel were available. Bus models confirm
software sequencing/state, not electrical timing. No target stack profiling,
formal proof, fuzzing campaign, or current RustSec/CVE database scan was performed.
The absence of a found leak/vulnerability is not a guarantee for all generic HAL
implementations or transitive dependencies. The review found no production heap
allocation in the five local drivers, but `no_std` alone does not establish that
arbitrary dependencies are allocation-free or memory-safe.

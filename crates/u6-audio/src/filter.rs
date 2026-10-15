//! The Amiga 500's analogue output stage, per stereo side, at the output sample rate.
//!
//! Values are the A500 board's parts: a fixed one-pole RC low-pass (360 Ω, 0.1 µF: 4421 Hz),
//! the "LED" filter switched by CIA-A port A bit 1 (a Sallen-Key low-pass, 10 kΩ / 10 kΩ,
//! 6800 pF / 3900 pF: 3091 Hz, Q 0.660) and the AC-coupling high-pass (1390 Ω, 22 µF: 5.2 Hz).

use std::f64::consts::PI;

/// Which output stage to model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterModel {
    /// Amiga 500: fixed low-pass, LED filter when switched on, AC coupling.
    #[default]
    A500,
    /// No analogue stage: the box-filtered DAC output as is.
    Raw,
}

const RC_LOWPASS_HZ: f64 = 4420.97;
const LED_HZ: f64 = 3090.53;
const LED_Q: f64 = 0.660;
const HIGHPASS_HZ: f64 = 5.20;

#[derive(Clone, Debug, Default, PartialEq)]
struct OnePole {
    a: f64,
    y: f64,
}

impl OnePole {
    fn new(hz: f64, rate: f64) -> Self {
        Self {
            a: 1.0 - (-2.0 * PI * hz / rate).exp(),
            y: 0.0,
        }
    }

    fn run(&mut self, x: f64) -> f64 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

/// RBJ biquad low-pass.
#[derive(Clone, Debug, Default, PartialEq)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn lowpass(hz: f64, q: f64, rate: f64) -> Self {
        let w = 2.0 * PI * (hz / rate).min(0.49);
        let alpha = w.sin() / (2.0 * q);
        let c = w.cos();
        let a0 = 1.0 + alpha;
        let b1 = (1.0 - c) / a0;
        Self {
            b: [b1 / 2.0, b1, b1 / 2.0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            ..Self::default()
        }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// One side's filter chain.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Side {
    rc: OnePole,
    led: Biquad,
    dc: OnePole,
}

impl Side {
    fn new(rate: f64) -> Self {
        Self {
            rc: OnePole::new(RC_LOWPASS_HZ, rate),
            led: Biquad::lowpass(LED_HZ, LED_Q, rate),
            dc: OnePole::new(HIGHPASS_HZ, rate),
        }
    }

    fn run(&mut self, x: f64, led_on: bool) -> f64 {
        let mut v = self.rc.run(x);
        // The LED filter's state keeps running while bypassed so switching it does not click.
        let led = self.led.run(v);
        if led_on {
            v = led;
        }
        v - self.dc.run(v)
    }
}

/// Both sides, rebuilt when the sample rate changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Output {
    rate: u32,
    sides: [Side; 2],
}

impl Output {
    pub(crate) fn run(
        &mut self,
        model: FilterModel,
        rate: u32,
        led_on: bool,
        lr: (f64, f64),
    ) -> (f64, f64) {
        if model == FilterModel::Raw {
            return lr;
        }
        if self.rate != rate {
            self.rate = rate;
            self.sides = [Side::new(f64::from(rate)), Side::new(f64::from(rate))];
        }
        let [l, r] = &mut self.sides;
        (l.run(lr.0, led_on), r.run(lr.1, led_on))
    }
}

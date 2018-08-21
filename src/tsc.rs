//! Touch sense controller

use rcc::AHB1;
use stm32l4::stm32l4x2::{TSC};
use gpio::gpiob::{PB4, PB5, PB6, PB7};
use gpio::{AF9, Alternate, Output, OpenDrain, PushPull};

#[derive(Debug)]
pub enum Event {
    /// Max count error
    MaxCountError,
    /// End of acquisition
    EndOfAcquisition
}

// TODO macro to impl all possible channel/sample pin combinations
pub trait SamplePin<TSC> {
    const GROUP: u32;
    const OFFSET: u32;
}
impl SamplePin<TSC> for PB4<Alternate<AF9, Output<OpenDrain>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 0;
}
impl SamplePin<TSC> for PB5<Alternate<AF9, Output<OpenDrain>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 1;
}
impl SamplePin<TSC> for PB6<Alternate<AF9, Output<OpenDrain>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 2;
}
impl SamplePin<TSC> for PB7<Alternate<AF9, Output<OpenDrain>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 3;
}

pub trait ChannelPin<TSC> {
    const GROUP: u32;
    const OFFSET: u32;
}
impl ChannelPin<TSC> for PB4<Alternate<AF9, Output<PushPull>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 0;
}
impl ChannelPin<TSC> for PB5<Alternate<AF9, Output<PushPull>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 1;
}
impl ChannelPin<TSC> for PB6<Alternate<AF9, Output<PushPull>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 2;
}
impl ChannelPin<TSC> for PB7<Alternate<AF9, Output<PushPull>>> {
    const GROUP: u32 = 2;
    const OFFSET: u32 = 3;
}


// TODO currently requires all the pins even if a user wants one channel, fix
pub struct Tsc<SPIN> {
    sample_pin: SPIN,
    // pins: PINS,
    tsc: TSC
}

impl<SPIN> Tsc<SPIN> {
    pub fn tsc(tsc: TSC, sample_pin: SPIN, ahb: &mut AHB1) -> Self
        where SPIN: SamplePin<TSC>
    {
        /* Enable the peripheral clock */
        ahb.enr().modify(|_, w| w.tscen().set_bit());
        ahb.rstr().modify(|_, w| w.tscrst().set_bit());
        ahb.rstr().modify(|_, w| w.tscrst().clear_bit());

        tsc.cr.write(|w| unsafe {
            w.ctph()
                .bits((1 << 28) as u8)
                .ctpl()
                .bits((1 << 24) as u8)
                .sse()
                .clear_bit()
                .pgpsc()
                .bits((2 << 12) as u8)
                .mcv()
                // 000: 255
                // 001: 511
                // 010: 1023
                // 011: 2047
                // 100: 4095
                // 101: 8191
                // 110: 16383
                .bits(0b101) // TODO make this value configurable
                .tsce()
                .set_bit()
        });

        // TODO allow configuration
        
        let bit_pos = SPIN::OFFSET + (4 * (SPIN::GROUP - 1));
        
        // Schmitt trigger hysteresis on sample IOs
        tsc.iohcr.write(|w| unsafe {
            w.bits(1 << bit_pos)
        });

        // Set the sampling pin
        tsc.ioscr.write(|w| unsafe { w.bits(1 << bit_pos) });
        
        // set the acquisitiuon groups based of the channel pins, stm32l432xx only has group 2
        tsc.iogcsr.write(|w| { w.g2e().set_bit() });

        // clear interrupt & flags
        tsc.icr.write(|w| { 
            w.eoaic().set_bit()
                .mceic().set_bit()
        });

        Tsc {
            tsc: tsc,
            sample_pin: sample_pin,
        }
    }

    /// Starts a charge acquisition
    pub fn start<PIN>(&self, _input: &mut PIN) 
        where PIN: ChannelPin<TSC>
    {
        // clear interrupt & flags
        self.tsc.icr.write(|w| { 
            w.eoaic().set_bit()
                .mceic().set_bit()
        });

        // discharge the caps ready for a new reading
        self.tsc.cr.modify(|_, w| {
            w.iodef().clear_bit()
        });

        let bit_pos = PIN::OFFSET + (4 * (PIN::GROUP - 1));

        // Set the channel pin
        self.tsc.ioccr.write(|w| unsafe {
            w.bits(1 << bit_pos)
        });

        self.tsc.cr.modify(|_, w| { w.start().set_bit() });
    }

    /// Blocks waiting for a acquisition to complete or for a Max Count Error
    pub fn acquire<PIN>(&self, input: &mut PIN) -> Result<u16, Event>
        where PIN: ChannelPin<TSC>
    {
        let bit_pos = PIN::OFFSET + (4 * (PIN::GROUP - 1));
        
        // disable Schmitt trigger hysteresis
        self.tsc.iohcr.write(|w| unsafe {
            w.bits(1 << bit_pos)
        });

        self.start(input);

        let result = loop {
            let isr = self.tsc.isr.read();
            if isr.eoaf().bit_is_set() {
                break Ok(self.tsc.iog2cr.read().cnt().bits())
            } else if isr.mcef().bit_is_set() {
                break Err(Event::MaxCountError)
            }
        };

        // re-enable Schmitt trigger hysteresis
        self.tsc.iohcr.write(|w| unsafe {
            w.bits(0 << bit_pos)
        });

        result
    }

    /// Enables an interrupt event
    pub fn listen(&mut self, event: Event){
        match event {
            Event::EndOfAcquisition => {
                self.tsc.ier.modify(|_, w| w.eoaie().set_bit());
            },
            Event::MaxCountError => {
                self.tsc.ier.modify(|_, w| w.mceie().set_bit());
            },
        }
    }

    /// Disables an interrupt event
    pub fn unlisten(&self, event: Event) {
        match event {
            Event::EndOfAcquisition => {
                self.tsc.ier.modify(|_, w| w.eoaie().clear_bit());
            },
            Event::MaxCountError => {
                self.tsc.ier.modify(|_, w| w.mceie().clear_bit());
            },
        }
    }

    /// Releases the TSC peripheral and associated pins
    pub fn free(self) -> (TSC, SPIN) {
        (self.tsc, self.sample_pin)
    }
}
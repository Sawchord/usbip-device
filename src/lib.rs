pub(crate) mod cmd;
pub(crate) mod debug;
pub(crate) mod endpoint;
pub(crate) mod handler;
pub(crate) mod op;
pub(crate) mod request;
pub(crate) mod response;

use crate::{
    endpoint::{ControlPipe, InPipe, OutPipe, PacketKind},
    handler::SocketHandler,
};
use std::sync::{Arc, Mutex, MutexGuard};
use usb_device::{
    Result as UsbResult, UsbDirection, UsbError,
    {
        bus::{PollResult, UsbBus},
        endpoint::{EndpointAddress, EndpointType},
    },
};

#[derive(Debug, Clone)]
/// The error type, used by this crate.
pub enum UsbIpError {
    /// The connection closed unexpectedly.
    ConnectionClosed,

    /// A packet was received but it was shorter than the 48 bytes required to parse the header.
    PkgTooShort(usize),

    /// A received packet contained a command, that is unknown to the USBIP specification.
    InvalidCommand(u16),

    /// A received packet had a status field set to an unknown status value.
    StatusNotOk(u32),
}

impl std::fmt::Display for UsbIpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionClosed => write!(f, "connection no longer exsists"),
            Self::PkgTooShort(len) => write!(f, "packet of length {} is to short to parse", len),
            Self::InvalidCommand(cmd) => write!(f, "unknown command: {}", cmd),
            Self::StatusNotOk(status) => write!(f, "received invalid status: {}", status),
        }
    }
}

impl std::error::Error for UsbIpError {}

const NUM_ENDPOINTS: usize = 8;

#[derive(Debug)]
pub(crate) struct UsbIpBusInner {
    pub handler: SocketHandler,
    pub control: Option<ControlPipe>,
    pub ins: [Option<InPipe>; NUM_ENDPOINTS],
    pub outs: [Option<OutPipe>; NUM_ENDPOINTS],
    pub device_address: u8,
    pub reset: bool,
    pub suspended: bool,
}

impl UsbIpBusInner {
    /// Creates a new UsbIpBusInner
    fn new() -> Self {
        Self {
            handler: SocketHandler::new(),
            control: None,
            ins: Default::default(),
            outs: Default::default(),
            device_address: 0,
            reset: true,
            suspended: false,
        }
    }

    /// Drops every transfer in flight, keeping the endpoints as allocated
    fn clear_transfers(&mut self) {
        self.control.iter_mut().for_each(ControlPipe::clear);
        self.ins.iter_mut().flatten().for_each(InPipe::clear);
        self.outs.iter_mut().flatten().for_each(OutPipe::clear);
    }

    /// Returns the first endpoint number that is free in the given direction
    fn next_available_endpoint(&self, direction: UsbDirection) -> Option<usize> {
        (1..NUM_ENDPOINTS).find(|&i| match direction {
            UsbDirection::In => self.ins[i].is_none(),
            UsbDirection::Out => self.outs[i].is_none(),
        })
    }

    fn control(&mut self) -> UsbResult<&mut ControlPipe> {
        self.control.as_mut().ok_or(UsbError::InvalidEndpoint)
    }

    fn pipe_in(&mut self, index: usize) -> UsbResult<&mut InPipe> {
        self.ins
            .get_mut(index)
            .and_then(Option::as_mut)
            .ok_or(UsbError::InvalidEndpoint)
    }

    fn pipe_out(&mut self, index: usize) -> UsbResult<&mut OutPipe> {
        self.outs
            .get_mut(index)
            .and_then(Option::as_mut)
            .ok_or(UsbError::InvalidEndpoint)
    }

    fn is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        match (ep_addr.index(), ep_addr.direction()) {
            (0, _) => self.control().is_ok_and(|control| control.is_stalled()),
            (index, UsbDirection::In) => self.pipe_in(index).is_ok_and(|pipe| pipe.is_stalled()),
            (index, UsbDirection::Out) => self.pipe_out(index).is_ok_and(|pipe| pipe.is_stalled()),
        }
    }

    /// Removes a URB the host gave up on, returning whether it was still pending
    fn unlink(&mut self, seqnum: u32) -> bool {
        self.control
            .iter_mut()
            .any(|control| control.unlink(seqnum))
            || self
                .ins
                .iter_mut()
                .flatten()
                .any(|pipe| pipe.unlink(seqnum))
            || self
                .outs
                .iter_mut()
                .flatten()
                .any(|pipe| pipe.unlink(seqnum))
    }
}

#[derive(Debug, Clone)]
/// An implementation of [`UsbBus`](https://docs.rs/usb-device/0.2.7/usb_device/bus/trait.UsbBus.html),
/// based on the Linux USBIP protocol.
pub struct UsbIpBus(Arc<Mutex<UsbIpBusInner>>);

impl UsbIpBus {
    /// Create a new [`UsbIpBus`].
    ///
    /// # Note
    /// There can only ever be one bus [`UsbIpBus`] device on the system, since
    /// it is blocking port 3240.
    ///
    /// # Panics
    /// If port 3240 is already in use.
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(UsbIpBusInner::new())))
    }

    fn lock(&self) -> MutexGuard<UsbIpBusInner> {
        self.0.lock().unwrap()
    }
}

impl Default for UsbIpBus {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbBus for UsbIpBus {
    fn alloc_ep(
        &mut self,
        ep_dir: UsbDirection,
        ep_addr: Option<EndpointAddress>,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval: u8,
    ) -> UsbResult<EndpointAddress> {
        let mut inner = self.lock();

        let index = match ep_addr {
            Some(addr) if addr.index() < NUM_ENDPOINTS => addr.index(),
            Some(_) => return Err(UsbError::InvalidEndpoint),
            None => inner
                .next_available_endpoint(ep_dir)
                .ok_or(UsbError::EndpointMemoryOverflow)?,
        };

        // Endpoint 0 is the one control endpoint, and both of its halves share one pipe
        match (ep_type, index) {
            (EndpointType::Control, 0) => {
                if inner.control.is_none() {
                    inner.control = Some(ControlPipe::new(max_packet_size));
                }
            }
            (EndpointType::Control, _) | (_, 0) => return Err(UsbError::InvalidEndpoint),
            _ => match ep_dir {
                UsbDirection::In if inner.ins[index].is_none() => {
                    inner.ins[index] = Some(InPipe::new(max_packet_size))
                }
                UsbDirection::Out if inner.outs[index].is_none() => {
                    inner.outs[index] = Some(OutPipe::new(max_packet_size))
                }
                _ => return Err(UsbError::InvalidEndpoint),
            },
        }

        log::debug!(
            "initialized endpoint {} {:?} as {:?} with interval {}",
            index,
            ep_dir,
            ep_type,
            interval
        );

        Ok(EndpointAddress::from_parts(index, ep_dir))
    }

    fn enable(&mut self) {
        log::info!("usb device is being enabled");
    }

    fn reset(&self) {
        let mut inner = self.lock();

        log::trace!("usb device is being reset");
        inner.clear_transfers();
        inner.suspended = false;
    }

    fn set_device_address(&self, addr: u8) {
        let mut inner = self.lock();

        log::info!("setting device address to {}", addr);
        inner.device_address = addr;
    }

    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> UsbResult<usize> {
        log::trace!("write request at endpoint {}", ep_addr.index());
        let mut inner = self.lock();

        // We can not write anything, as long as there is no connection
        if !inner.handler.is_connected() {
            return Err(UsbError::WouldBlock);
        }

        let completion = match ep_addr.index() {
            0 => inner.control()?.write(buf)?,
            index => inner.pipe_in(index)?.write(buf)?,
        };
        inner.send(completion);

        Ok(buf.len())
    }

    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> UsbResult<usize> {
        log::trace!("read request at endpoint {}", ep_addr.index());
        let mut inner = self.lock();

        match ep_addr.index() {
            0 => inner.control()?.read(buf),
            index => {
                let (result, completion) = inner.pipe_out(index)?.read(buf);
                inner.send(completion);
                result
            }
        }
    }

    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        let mut inner = self.lock();

        if stalled != inner.is_stalled(ep_addr) {
            log::debug!(
                "setting endpoint {:?} to stalled state {}",
                ep_addr,
                stalled
            );
        }

        let completions = match (ep_addr.index(), ep_addr.direction()) {
            (0, _) => inner
                .control()
                .map(|control| control.set_stalled(stalled).into_iter().collect())
                .unwrap_or_default(),
            (index, UsbDirection::In) => inner
                .pipe_in(index)
                .map(|pipe| pipe.set_stalled(stalled))
                .unwrap_or_default(),
            (index, UsbDirection::Out) => inner
                .pipe_out(index)
                .map(|pipe| pipe.set_stalled(stalled))
                .unwrap_or_default(),
        };
        inner.send(completions);
    }

    fn is_stalled(&self, ep_addr: EndpointAddress) -> bool {
        self.lock().is_stalled(ep_addr)
    }

    fn suspend(&self) {
        let mut inner = self.lock();

        log::info!("suspending device");
        if inner.suspended {
            log::warn!("supending already suspended device");
        }

        inner.suspended = true;
    }

    fn resume(&self) {
        let mut inner = self.lock();

        log::info!("resuming device");
        if !inner.suspended {
            log::warn!("resuming already active device");
        }

        inner.suspended = false;
    }

    fn poll(&self) -> PollResult {
        let mut inner = self.lock();
        log::trace!("usb device is being polled");

        inner.handle_socket();

        if inner.reset {
            log::trace!("device is in reset state");
            return PollResult::Reset;
        }

        if inner.suspended {
            log::trace!("device is suspended");
            return PollResult::Suspend;
        }

        let mut ep_in_complete: u16 = 0;
        let mut ep_out: u16 = 0;
        let mut ep_setup: u16 = 0;

        if let Some(control) = inner.control.as_mut() {
            ep_in_complete |= control.take_sent() as u16;
            match control.next_packet() {
                Some(PacketKind::Setup) => ep_setup |= 1,
                Some(PacketKind::Data) => ep_out |= 1,
                None => {}
            }
        }

        for i in 1..NUM_ENDPOINTS {
            if let Some(pipe) = inner.outs[i].as_ref() {
                ep_out |= (pipe.has_packet() as u16) << i;
            }
            if let Some(pipe) = inner.ins[i].as_mut() {
                ep_in_complete |= (pipe.take_sent() as u16) << i;
            }
        }

        PollResult::Data {
            ep_out,
            ep_in_complete,
            ep_setup,
        }
    }
}

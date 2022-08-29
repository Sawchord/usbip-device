pub(crate) mod cmd;
pub(crate) mod debug;
pub(crate) mod handler;
pub(crate) mod op;
pub(crate) mod request;
pub(crate) mod response;

use crate::{cmd::UsbIpHeader, handler::SocketHandler, request::UsbIpCmdSubmit};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};
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

#[derive(Debug, Clone)]
pub(crate) struct Pipe {
    pub data: VecDeque<Vec<u8>>,
    pub ty: EndpointType,
    pub max_packet_size: u16,
    #[allow(dead_code)]
    pub interval: u8,
}

impl Pipe {
    /// Checks, whether the endpoint contains a full transaction
    /// (terminated by a short packet) and is ready to send it.
    pub fn is_rts(&self) -> bool {
        match self.data.back() {
            // If there is no data pending, we are not ready to send
            None => false,
            Some(val) => {
                if self.ty != EndpointType::Control {
                    true
                } else {
                    // Control Endpoint use packet transactions.
                    // Therefore, we must wait until a transaction is complete before we return the packets
                    val.len() < self.max_packet_size as usize
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Endpoint {
    pub(crate) pipe_in: Option<Pipe>,
    pub(crate) pipe_out: Option<Pipe>,
    pub(crate) pending_ins: VecDeque<(UsbIpHeader, UsbIpCmdSubmit, Vec<u8>)>,
    pub(crate) stalled: bool,
    pub(crate) setup_flag: bool,
    pub(crate) in_complete_flag: bool,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            pipe_in: None,
            pipe_out: None,
            pending_ins: VecDeque::new(),
            stalled: true,
            setup_flag: false,
            in_complete_flag: false,
        }
    }
}

impl Endpoint {
    /// Returns the input pipe of this endpoint
    fn get_in(&mut self) -> UsbResult<&mut Pipe> {
        self.pipe_in.as_mut().ok_or(UsbError::InvalidEndpoint)
    }

    /// Returns the output pipe of this endpoint
    fn get_out(&mut self) -> UsbResult<&mut Pipe> {
        self.pipe_out.as_mut().ok_or(UsbError::InvalidEndpoint)
    }

    /// Checks, whether the input pipe is ready to send data back to the host.
    fn is_rts(&self) -> bool {
        match self.pipe_in {
            None => false,
            Some(ref pipe) => pipe.is_rts(),
        }
    }

    /// Processes an unlink and removes the pending packet on this endpoint.
    ///
    /// # Returns
    /// - `true` if pending urb was removed
    /// - `false` if it was not found
    // NOTE: This is super inefficient, use linked lists, as soon as linked_list_remove stabilizes
    fn unlink(&mut self, seqnum: u32) -> bool {
        let old_len = self.pending_ins.len();

        self.pending_ins = self
            .pending_ins
            .drain(..)
            .filter(|(header, _, _)| header.seqnum != seqnum)
            .collect();

        // If the length is the same as before, we have not changed anything
        // and return false
        old_len != self.pending_ins.len()
    }
}

#[derive(Debug)]
pub(crate) struct UsbIpBusInner {
    pub handler: SocketHandler,
    pub endpoint: [Endpoint; NUM_ENDPOINTS],
    pub device_address: u8,
    pub reset: bool,
    pub suspended: bool,
}

impl UsbIpBusInner {
    /// Creates a new UsbIpBusInner
    fn new() -> Self {
        Self {
            handler: SocketHandler::new(),
            endpoint: <[Endpoint; NUM_ENDPOINTS]>::default(),
            device_address: 0,
            reset: true,
            suspended: false,
        }
    }

    /// Resets the handler to the state, in which it acts like it is new
    fn reset(&mut self) {
        self.endpoint = <[Endpoint; NUM_ENDPOINTS]>::default();
        self.reset = true;
        self.suspended = false;
    }

    /// Returns the first enpoint, that is not already initialized or `None`,
    /// if all are already in use.
    fn next_available_endpoint(&self, direction: UsbDirection) -> Option<usize> {
        match direction {
            UsbDirection::In => {
                for i in 1..NUM_ENDPOINTS {
                    if self.endpoint[i].pipe_in.is_none() {
                        return Some(i);
                    }
                }
            }
            UsbDirection::Out => {
                for i in 1..NUM_ENDPOINTS {
                    if self.endpoint[i].pipe_out.is_none() {
                        return Some(i);
                    }
                }
            }
        }

        None
    }

    /// Returns the requested endpoint if it exists and
    /// [`UsbError::InvalidEndpoint`] otherwise.
    fn get_endpoint(&mut self, ep: usize) -> UsbResult<&mut Endpoint> {
        //let ep_addr = ep.index();

        if ep >= NUM_ENDPOINTS {
            log::error!("attempt to access out-of-bounds endpoint {:?}", ep);
            return Err(UsbError::InvalidEndpoint);
        }

        Ok(&mut self.endpoint[ep])
    }

    /// Processes an unlink and removes the pending packet.
    ///
    /// # Returns
    /// - `true` if pending urb was removed
    /// - `false` if it was not found
    fn unlink(&mut self, seqnum: u32) -> bool {
        for i in 0..NUM_ENDPOINTS {
            if self.endpoint[i].unlink(seqnum) {
                return true;
            }
        }

        false
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

        // Get the endpoint to initialize
        let endpoint_index = match ep_addr {
            Some(addr) => {
                if addr.index() < NUM_ENDPOINTS {
                    addr.index()
                } else {
                    return Err(UsbError::InvalidEndpoint);
                }
            }
            None => inner
                .next_available_endpoint(ep_dir)
                .ok_or(UsbError::EndpointMemoryOverflow)?,
        };

        let endpoint = &mut inner.endpoint[endpoint_index as usize];

        // check endpoint allocation here
        let maybe_pipe = match ep_dir {
            UsbDirection::In => endpoint.get_in(),
            UsbDirection::Out => endpoint.get_out(),
        };

        // we want to get an invalid enpoint, otherwise the requested endpoint
        // was already allocated
        match maybe_pipe {
            Err(UsbError::InvalidEndpoint) => (),
            Ok(_) => return Err(UsbError::InvalidEndpoint),
            Err(_) => return Err(UsbError::InvalidEndpoint),
        }

        // initialize the endpoint
        let pipe = Pipe {
            data: VecDeque::new(),
            ty: ep_type,
            max_packet_size,
            interval,
        };
        match ep_dir {
            UsbDirection::In => endpoint.pipe_in = Some(pipe),
            UsbDirection::Out => endpoint.pipe_out = Some(pipe),
        }

        log::debug!(
            "initialized new endpoint {:?} as address {:?}",
            endpoint,
            endpoint_index
        );

        Ok(EndpointAddress::from_parts(endpoint_index as usize, ep_dir))
    }

    fn enable(&mut self) {
        log::info!("usb device is being enabled");
    }

    fn reset(&self) {
        let mut inner = self.lock();

        // Skip if we are already in reset state
        if inner.reset {
            return;
        }

        inner.reset();
        log::debug!("usb device is being reset");
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

        // Get the endpoint
        let ep = inner.get_endpoint(ep_addr.index())?;

        // NOTE: This is a hack to allow driving the setup packets in a transactional way
        // while the rest of the packets use packet logic
        if ep_addr.index() == 0 {
            ep.in_complete_flag = true;
        }

        let pipe = ep.get_in()?;

        // If there is data waiting in the output buffer, we need to wait
        if pipe.is_rts() {
            if ep_addr.index() == 0 {
                ep.in_complete_flag = false;
            }
            return Err(UsbError::WouldBlock);
        }

        pipe.data.push_back(buf.to_vec());

        // we attempt to service in packets, if we have them available
        if pipe.is_rts() {
            inner.try_send_pending(ep_addr.index());
        }

        Ok(buf.len())
    }

    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> UsbResult<usize> {
        log::trace!("read request at endpoint {}", ep_addr.index());
        let mut inner = self.lock();
        let ep = inner.get_endpoint(ep_addr.index())?;
        let pipe = ep.get_out()?;

        // Try to get data
        let data = match pipe.data.pop_front() {
            None => {
                log::trace!("no data available at endpoint");
                return Err(UsbError::WouldBlock);
            }
            Some(data) => data,
        };

        if buf.len() < data.len() {
            buf.copy_from_slice(&data[..buf.len()]);
        } else {
            buf[..data.len()].copy_from_slice(&data);
        }
        Ok(data.len())
    }

    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        let mut inner = self.lock();

        let endpoint = match inner.get_endpoint(ep_addr.index()) {
            Ok(endpoint) => endpoint,
            _ => return,
        };

        if endpoint.stalled != stalled {
            log::debug!(
                "setting endpoint {:?} to stalled state {}",
                ep_addr,
                stalled
            );
        }
        endpoint.stalled = stalled;
    }

    fn is_stalled(&self, ep_addr: EndpointAddress) -> bool {
        let mut inner = self.lock();

        let endpoint = match inner.get_endpoint(ep_addr.index()) {
            Ok(endpoint) => endpoint,
            _ => return false,
        };

        endpoint.stalled
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

        let mut ep_in: u16 = 0;
        let mut ep_out: u16 = 0;
        let mut ep_setup: u16 = 0;

        for i in (0..NUM_ENDPOINTS).into_iter().rev() {
            ep_in <<= 1;
            ep_out <<= 1;
            ep_setup <<= 1;

            let ep = &mut inner.endpoint[i];

            // Check for pending output
            if let Some(ref pipe) = ep.pipe_out {
                if !pipe.data.is_empty() {
                    ep_out |= 1;
                }
            }

            if ep.in_complete_flag {
                ep.in_complete_flag = false;
                ep_in |= 1;
            }

            if ep.setup_flag {
                ep.setup_flag = false;
                ep_setup |= 1;
            }
        }

        PollResult::Data {
            ep_out,
            ep_in_complete: ep_in,
            ep_setup,
        }
    }
}

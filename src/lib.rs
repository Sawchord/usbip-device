#![allow(dead_code)]

pub(crate) mod cmd;
pub(crate) mod handler;
pub(crate) mod op;

use crate::handler::SocketHandler;
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

const NUM_ENDPOINTS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub(crate) struct EndpointConf {
    pub ty: EndpointType,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    pub(crate) in_ep: Option<EndpointConf>,
    pub(crate) out_ep: Option<EndpointConf>,
    pub(crate) stalled: bool,
    pub(crate) in_buf: VecDeque<Vec<u8>>,
    pub(crate) out_buf: VecDeque<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct UsbIpBusInner {
    pub endpoint: [Endpoint; NUM_ENDPOINTS],
    pub device_address: u8,
    pub reset: bool,
    pub suspended: bool,
}

impl UsbIpBusInner {
    /// Returns the first enpoint, that is not already initialized or `None`,
    /// if all are already in use.
    fn next_available_endpoint(&self, direction: UsbDirection) -> Option<usize> {
        match direction {
            UsbDirection::In => {
                for i in 1..NUM_ENDPOINTS {
                    if self.endpoint[i].in_ep.is_none() {
                        return Some(i);
                    }
                }
            }
            UsbDirection::Out => {
                for i in 1..NUM_ENDPOINTS {
                    if self.endpoint[i].out_ep.is_none() {
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
}

#[derive(Debug, Clone)]
pub struct UsbIpBus(Arc<Mutex<UsbIpBusInner>>);

impl UsbIpBus {
    /// Create a new [`UsbIpBus`].
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let new_bus = Self(Arc::new(Mutex::new(UsbIpBusInner {
            endpoint: <[Endpoint; NUM_ENDPOINTS]>::default(),
            device_address: 0,
            reset: true,
            suspended: false,
        })));

        SocketHandler::run(new_bus.clone());
        Ok(new_bus)
    }

    fn lock(&self) -> MutexGuard<UsbIpBusInner> {
        self.0.lock().unwrap()
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

        // if address is already in use,
        // if !endpoint.is_none() {
        //     return Err(UsbError::InvalidEndpoint);
        // }

        // initialize the endpoint
        let ep_conf = EndpointConf {
            ty: ep_type,
            max_packet_size,
            interval,
        };
        match ep_dir {
            UsbDirection::In => endpoint.in_ep = Some(ep_conf),
            UsbDirection::Out => endpoint.out_ep = Some(ep_conf),
        }

        log::info!(
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
        log::debug!("usb device is being reset");
    }

    fn set_device_address(&self, addr: u8) {
        let mut inner = self.lock();

        log::info!("setting device address to {}", addr);
        inner.device_address = addr;
    }

    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> UsbResult<usize> {
        log::debug!("write request at endpoint {}", ep_addr.index());
        let mut inner = self.lock();
        let ep = inner.get_endpoint(ep_addr.index())?;

        // Check that the buffer fits the max packet lentgth?
        ep.in_buf.push_back(buf.to_vec());

        Ok(buf.len())
    }

    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> UsbResult<usize> {
        log::debug!("read request at endpoint {}", ep_addr.index());
        let mut inner = self.lock();
        let ep = inner.get_endpoint(ep_addr.index())?;

        // Try to get data
        let data = match ep.out_buf.pop_front() {
            None => {
                log::debug!("no data available at endpoint");
                return Err(UsbError::WouldBlock);
            }
            Some(data) => data,
        };

        // Check that the read buffer is large enough
        if buf.len() < data.len() {
            log::warn!(
                "buffer of lenth {} to small for data of length {}",
                buf.len(),
                data.len()
            );
            ep.out_buf.push_front(data);
            Err(UsbError::BufferOverflow)
        } else {
            buf.copy_from_slice(&data);
            Ok(data.len())
        }
    }

    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        let mut inner = self.lock();

        let endpoint = match inner.get_endpoint(ep_addr.index()) {
            Ok(endpoint) => endpoint,
            _ => return,
        };

        log::info!(
            "setting endpoint {:?} to stalled state {}",
            ep_addr,
            stalled
        );
        endpoint.stalled = true;
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
        let inner = self.lock();
        log::debug!("usb device is being polled");

        if inner.reset {
            log::debug!("device is in reset state");
            return PollResult::Reset;
        }

        if inner.suspended {
            log::debug!("device is suspended");
            return PollResult::Suspend;
        }

        let mut ep_in: u16 = 0;
        let mut ep_out: u16 = 0;

        for i in NUM_ENDPOINTS..0 {
            let ep = &inner.endpoint[i];
            if !ep.out_buf.is_empty() {
                ep_out &= 1;
            }

            // TODO: Implement in_complete
            // TODO: Implement setup
            ep_in <<= 1;
            ep_out <<= 1;
        }

        PollResult::Data {
            ep_out,
            ep_in_complete: ep_in,
            ep_setup: 0,
        }
    }
}

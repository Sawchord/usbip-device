mod usbip;

use std::{
    net::{IpAddr, UdpSocket},
    sync::{Mutex, MutexGuard},
};
use usb_device::{
    Result as UsbResult, UsbDirection, UsbError,
    {
        bus::{PollResult, UsbBus},
        endpoint::{EndpointAddress, EndpointType},
    },
};

const NUM_ENDPOINTS: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct Endpoint {
    direction: UsbDirection,
    ty: EndpointType,
    max_packet_size: u16,
    interval: u8,
    stalled: bool,
    // TODO: Input and Output buffer
}

#[derive(Debug)]
pub struct UsbIpBusInner {
    endpoint: [Option<Endpoint>; NUM_ENDPOINTS],
    socket: UdpSocket,
    device_address: u8,
    suspended: bool,
}

impl UsbIpBusInner {
    /// Returns the first enpoint, that is not already initialized or `None`,
    /// if all are already in use.
    fn next_available_endpoint(&self) -> Option<usize> {
        for i in 1..NUM_ENDPOINTS {
            if self.endpoint[i].is_none() {
                return Some(i);
            }
        }

        None
    }

    /// Returns the requested endpoint if it exists and
    /// [`UsbError::InvalidEndpoint`] otherwise.
    fn get_endpoint(&mut self, ep: EndpointAddress) -> UsbResult<&mut Endpoint> {
        let ep_addr = ep.index();

        if ep_addr >= NUM_ENDPOINTS {
            log::error!("attempt to access out-of-bounds endpoint {:?}", ep);
            return Err(UsbError::InvalidEndpoint);
        }

        match self.endpoint[ep_addr] {
            Some(ref mut endpoint) => Ok(endpoint),
            None => return Err(UsbError::InvalidEndpoint),
        }
    }
}

#[derive(Debug)]
pub struct UsbIpBus(Mutex<UsbIpBusInner>);

impl UsbIpBus {
    /// Create a new [`UsbIpBus`].
    pub fn new(addr: IpAddr, port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self(Mutex::new(UsbIpBusInner {
            endpoint: [None; NUM_ENDPOINTS],
            socket: UdpSocket::bind((addr, port))?,
            device_address: 0,
            suspended: false,
        })))
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
        // TODO: Check matching dir in address and ep_dir
        let endpoint_index = match ep_addr {
            Some(addr) => {
                if addr.index() < NUM_ENDPOINTS {
                    addr.index()
                } else {
                    return Err(UsbError::InvalidEndpoint);
                }
            }
            None => inner
                .next_available_endpoint()
                .ok_or(UsbError::EndpointMemoryOverflow)?,
        };

        let endpoint = &mut inner.endpoint[endpoint_index as usize];

        // if address is already in use,
        // if !endpoint.is_none() {
        //     return Err(UsbError::InvalidEndpoint);
        // }

        // initialize the endpoint
        *endpoint = Some(Endpoint {
            direction: ep_dir,
            ty: ep_type,
            max_packet_size,
            interval,
            stalled: false,
        });
        log::info!(
            "initialized new endpoint {:?} as address {:?}",
            endpoint,
            endpoint_index
        );

        Ok(EndpointAddress::from_parts(endpoint_index as usize, ep_dir))
    }

    fn enable(&mut self) {
        todo!()
    }

    fn reset(&self) {
        todo!()
    }

    fn set_device_address(&self, addr: u8) {
        let mut inner = self.lock();

        log::info!("setting device address to {}", addr);
        inner.device_address = addr;
    }

    fn write(&self, _ep_addr: EndpointAddress, _buf: &[u8]) -> UsbResult<usize> {
        todo!()
    }

    fn read(&self, _ep_addr: EndpointAddress, _buf: &mut [u8]) -> UsbResult<usize> {
        todo!()
    }

    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        let mut inner = self.lock();

        let endpoint = match inner.get_endpoint(ep_addr) {
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

        let endpoint = match inner.get_endpoint(ep_addr) {
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

        if inner.suspended {
            return PollResult::Suspend;
        }
        todo!()
    }
}

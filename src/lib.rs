#![allow(unused_variables)]

use std::{
    net::{IpAddr, UdpSocket},
    sync::{Mutex, MutexGuard},
};
use usb_device::{
    Result as UsbResult, UsbDirection,
    {
        bus::{PollResult, UsbBus},
        endpoint::{EndpointAddress, EndpointType},
    },
};

// TODO: Endpoint struct

#[derive(Debug)]
pub struct UsbIpBusInner {
    socket: UdpSocket,
    device_address: u8,
}

#[derive(Debug)]
pub struct UsbIpBus(Mutex<UsbIpBusInner>);

impl UsbIpBus {
    pub fn new(addr: IpAddr, port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self(Mutex::new(UsbIpBusInner {
            socket: UdpSocket::bind((addr, port))?,
            device_address: 0,
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
        todo!()
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

    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> UsbResult<usize> {
        todo!()
    }

    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> UsbResult<usize> {
        todo!()
    }

    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        todo!()
    }

    fn is_stalled(&self, ep_addr: EndpointAddress) -> bool {
        todo!()
    }

    fn suspend(&self) {
        todo!()
    }

    fn resume(&self) {
        todo!()
    }

    fn poll(&self) -> PollResult {
        todo!()
    }
}

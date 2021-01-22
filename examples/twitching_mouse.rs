use usb_device::{bus::UsbBusAllocator, prelude::*};
use usbd_hid::{
   descriptor::{generator_prelude::*, MouseReport},
   hid_class::HIDClass,
};
use usbip_device::UsbIpBus;

fn main() -> Result<(), Box<dyn std::error::Error>> {
   pretty_env_logger::init();

   log::info!("initializing allocator");

   let bus_allocator = UsbBusAllocator::new(UsbIpBus::new()?);
   let mut usb_hid = HIDClass::new(&bus_allocator, MouseReport::desc(), 60);
   let mut usb_bus = UsbDeviceBuilder::new(&bus_allocator, UsbVidPid(0x16c0, 0x27dd))
      //.manufacturer("Fake company")
      //.product("Twitchy Mousey")
      //.serial_number("TEST")
      .device_class(0xEF)
      .build();

   let mut cnt = 0;

   loop {
      if !usb_bus.poll(&mut [&mut usb_hid]) {
         cnt += 5;
         std::thread::sleep(std::time::Duration::from_millis(5));
      }

      if cnt == 1000 {
         usb_hid
            .push_input(&MouseReport {
               x: 0,
               y: 4,
               buttons: 0,
            })
            .unwrap();

         cnt = 0;
      }
   }
}

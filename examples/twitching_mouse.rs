use usb_device::{bus::UsbBusAllocator, prelude::*};
use usbd_hid::{
   descriptor::{generator_prelude::*, MouseReport},
   hid_class::HIDClass,
};
use usbip_device::UsbIpBus;

#[allow(dead_code)]
const DESC: [u8; 52] = [
   0x05, 0x01, // Usage Page (Generic Desktop)
   0x09, 0x02, // Usage (Mouse)
   0xa1, 0x01, // Collection (Application)
   0x09, 0x01, //   Usage (Pointer)
   0xa1, 0x00, //   Collection (Physical)
   0x05, 0x09, //     Usage Page (Button)
   0x19, 0x01, //     Usage Minimum (1)
   0x29, 0x03, //     Usage Maximum (3)
   0x15, 0x00, //     Logical Minimum (0)
   0x25, 0x01, //     Logical Maximum (1)
   0x95, 0x03, //     Report Count (3)
   0x75, 0x01, //     Report Size (1)
   0x81, 0x02, //     Input (Data, Variable, Absolute)
   0x95, 0x01, //     Report Count (1)
   0x75, 0x05, //     Report Size (5)
   0x81, 0x01, //     Input (Constant)
   0x05, 0x01, //     Usage Page (Generic Desktop)
   0x09, 0x30, //     Usage (X)
   0x09, 0x31, //     Usage (Y)
   0x09, 0x38, //     Usage (Wheel)
   0x15, 0x81, //     Logical Minimum (-0x7f)
   0x25, 0x7f, //     Logical Maximum (0x7f)
   0x75, 0x08, //     Report Size (8)
   0x95, 0x03, //     Report Count (3)
   0x81, 0x06, //     Input (Data, Variable, Relative)
   0xc0, //         End Collection
   0xc0, //       End Collection
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
   pretty_env_logger::init();

   log::info!("initializing allocator");

   let bus_allocator = UsbBusAllocator::new(UsbIpBus::new()?);
   let mut usb_hid = HIDClass::new(&bus_allocator, MouseReport::desc(), 5);
   //let mut usb_hid = HIDClass::new(&bus_allocator, &DESC, 5);
   let mut usb_bus = UsbDeviceBuilder::new(&bus_allocator, UsbVidPid(0x16c0, 0x27dd))
      //.manufacturer("Fake company")
      //.product("Twitchy Mousey")
      //.serial_number("TEST")
      .device_class(0xEF)
      .build();

   let mut cnt = 0;

   loop {
      std::thread::sleep(std::time::Duration::from_millis(5));
      cnt += 5;

      if cnt == 1000 {
         let _ = usb_hid.push_input(&MouseReport {
            x: 0,
            y: 4,
            buttons: 0,
         });

         cnt = 0;
      }

      usb_bus.poll(&mut [&mut usb_hid]);
   }
}

use serde::{Deserialize, Serialize};
#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
struct UsbIpCmdSubmit {
   command: u32,
   seqnum: u32,
   devid: u32,
   direction: u32,
   ep: u32,
   transfer_flags: u32,
   transfer_buffer_length: u32,
   start_frame: u32,
   number_of_packets: u32,
   interval: u32,
   setup: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
struct UsbIpRetSubmit {
   command: u32,
   seqnum: u32,
   devid: u32,
   direction: u32,
   ep: u32,
   status: u32,
   actual_length: u32,
   start_frame: u32,
   number_of_packets: u32,
   error_count: u32,
   setup: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Deserialize, Serialize)]
struct UsbIpUnlinkRequest {
   command: u32,
   seqnum: u32,
   devid: u32,
   direction: u32,
   ep: u32,
   transfer_flag: u32,
   transfer_buffer_length: u32,
   start_frame: u32,
   number_of_packets: u32,
   interval: u32,
   setup: u64,
}

#[repr(C)]
#[derive(Debug, Clone)]
struct OprepDevList {
   //header: UsbIpHeader,
   exported_device: u32,
   usb_path: String,
   bus_id: String,
   busnum: u32,
   devnum: u32,
   speed: u32,
   id_vendor: u16,
   id_product: u16,
   bcd_device: u16,
   device_class: u8,
   device_subclass: u8,
   device_protocol: u8,
   configuration_value: u8,
   num_configurations: u8,
   num_interfaces: u8,
   interface_class: u8,
   interface_subclass: u8,
   interface_protocol: u8,
   align: u8,
}

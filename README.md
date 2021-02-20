[1]: https://docs.rs/usb-device/0.2.7/usb_device/
[2]: https://docs.rs/usb-device/0.2.7/usb_device/device/struct.UsbDevice.html
[3]: https://docs.rs/usb-device/0.2.7/usb_device/class/trait.UsbClass.html

# Usb-Hid Driver

This is an implementation of [`usb-device`][1] using USBIP server.

## Note

This crate is **not** intended to be used in production ever.
It's purpose is to ease development of new [`UsbClass`][3] or [`UsbDevice`][2] implementations or to emulate USB devices for easier embedded application development.

## Usage

The usbip client can be stared in the following way:

```bash
sudo apt-get install linux-tools-generic
```

Then, start the application.

```bash
cargo run --example serial_echo
```

and then start the USBIP client.

```bash
// Start the vhci driver
sudo modprobe vhci-hcd

// List available devices
usbip list -r "localhost" 

// Attach USB device
usbip attach -r "localhost" -b "1-1"

// List connected devices
usbip port

// Detach device
usbip detach -p "port"
```

## Known Bugs

This is a very alpha software, which still has a lot of quirks to be worked out.

- When using HID, the connection always fails the first time and usually works the second time. It is not entirely clear, why.

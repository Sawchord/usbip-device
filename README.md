[1]: https://docs.rs/usb-device/0.2.7/usb_device/
[3]: https://docs.rs/usb-device/0.2.7/usb_device/class/trait.UsbClass.html
[4]: https://docs.rs/usb-device/0.2.7/usb_device/bus/trait.UsbBus.html

[doc-badge]: https://docs.rs/usbip-device/badge.svg
[doc-link]: https://docs.rs/usbip-device

[crates-badge]: https://img.shields.io/crates/v/usbip-device
[crates-link]: https://crates.io/crates/usbip-device

[apache2-license]: https://spdx.org/licenses/Apache-2.0.html
[mit-license]: https://spdx.org/licenses/MIT.htm

# Usb-Hid Driver

[![crates.io][crates-badge]][crates-link]
[![Documentation][doc-badge]][doc-link]

This is an implementation of the [`UsbBus`][4] trait of [`usb-device`][1], simulating a USB device as a USBIP server.

## Note

This crate is **not** intended to be used in production ever.
It's purpose is to ease development of new [`UsbClass`][3] implementation or to emulate USB devices for easier embedded application development.

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

Depending on you machine setup, you might need do `sudo`.

## Known Bugs

This is a very alpha software, which still has a lot of quirks to be worked out.

- When using HID, the connection always fails the first time and usually works the second time. It is not entirely clear, why.

## License

[Apache-2.0][apache2-license] or [MIT][mit-license].

# Usb-Hid Driver

This is an implementation of `usb-device` using USBIP server.

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

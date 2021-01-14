# Usb-Hid Driver

This is an implementation of `usb-device` using USBIP server.

The usbip client can be stared in the following way:
```
sudo apt-get install linux-tools-generic
```

Then, start the application.

```
cargo run --example serial_echo
```

and then start the USBIP client.

```
// List available devices
usbip list -r "ip-address" 

// Attach USB device
usbip attach -r "ip-address" -b "bus-ID"

// List connected devices
usbip port

// Detach device
usbip detach -p "port"
```
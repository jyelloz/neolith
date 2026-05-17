```
_  _ ____ ____ _    _ ___ _  _
|\ | |___ |  | |    |  |  |__|
| \| |___ |__| |___ |  |  |  |
```

# neolith

An attempt at an easy-to-understand
[Hotline](https://en.wikipedia.org/wiki/Hotline_Communications) protocol
implementation along with a reference client and server.

## Status

### What is currently implemented?

- Protocol frame serialization/deserialization
    - Client/Server Handshake
    - Transactions
- Many of the higher level protocol concepts such as:
    - Login
    - Set user name info
    - Send/receive chat
    - Read/post non-threaded news
    - many more...
- A flat-file user account database
    - 1 TOML file per user in a single directory
    - Interactive terminal-interface [user data editor](src/bin/nlserver-edit-user.rs)
- AppleDouble filesystem backend for Mac Resource Forks and Finder metadata
    - Read-only file browsing backed by a UNIX filesystem subtree
    - Single-file downloads/uploads with without resume support
- A very simple, insecure, and incoherent [demo server](src/bin/nlserver.rs)
    - Logins are enforced
    - Chat messaging
    - Broadcast messaging
    - Private chat rooms
    - Instant messaging
    - Non-threaded news
        - Currently stored in-memory only
    - Online *User Account* administration
        - Currently in-memory edits only, changes do not sync back to disk

### What is in progress?

- Server-side
    - File transfer

### What is not implemented?

- System-wide
    - Customizable text encoding
- Server-side
    - Folder Transfer
    - Download/Upload resumption
    - File Transfer queueing
    - File Transfer throttling
    - **User** Permission enforcement
    - File ~manipulation~ (move/delete/set info)
    - Communication with Trackers
- Client
    - Anything

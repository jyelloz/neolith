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
- A very simple, insecure, and incoherent [demo server](src/bin/nlserver.rs)
    - Logins are enforced
    - Filesystem interface with AppleDouble support for resource forks and
    most useful Finder metadata
        - Read-only file browsing backed by a UNIX filesystem subtree
        - Single-file downloads/uploads with Mac file support, without resume
        support
    - Chat messaging
    - Broadcast messaging
    - Private chat rooms
    - Instant messaging

### What is in progress?

- Server-side
    - File transfer

### What is not implemented?

- Server-side
    - Folder Transfer
    - Download/Upload resumption
    - User Permission enforcement
    - Online User administration
    - File manipulation (move/delete/set info)
    - Well-designed state machines for connections
    - A good dispatch mechanism for transaction receipt
    - A good model for request-reply sequences
    - Communication with Trackers
- Client
    - Anything

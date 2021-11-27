# neolith

```
_  _ ____ ____ _    _ ___ _  _
|\ | |___ |  | |    |  |  |__|
| \| |___ |__| |___ |  |  |  |
```

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
    - Read non-threaded news
    - many more...
- A very simple, insecure, and incoherent [demo server](src/bin/nlserver.rs).
    - Unrestricted logins
    - Read-only file browsing backed by a UNIX filesystem subtree
    - Chat messaging
    - Broadcast messaging
    - Private chat rooms
    - Instant messaging

### What is not implemented?

- Server-side
    - Permissions
    - User administration
    - File/folder transfer
    - File manipulation (move/delete/set info)
    - News posting
    - Well-designed state machines for connections
    - A good dispatch mechanism for transaction receipt.
    - A good model for request-reply sequences.
    - More ergonomic declaration and (de-)serialization of each protocol struct.
    - Communication with Trackers
- Client
    - Anything

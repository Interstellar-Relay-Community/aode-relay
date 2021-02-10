# AodeRelay
_A simple and efficient activitypub relay_

### Usage
To simply run the server, the command is as follows
```bash
$ ./relay
```

To learn about any other tasks, the `--help` flag can be passed
```bash
$ ./relay --help
relay 0.2.0
An activitypub relay

USAGE:
    relay [FLAGS] [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -u, --undo       Undo allowing or blocking domains
    -V, --version    Prints version information

OPTIONS:
    -a <allowed>...        A list of domains that should be allowed
    -b <blocks>...         A list of domains that should be blocked
```

To add domains to the blocklist, use the `-b` flag and pass a list of domains
```bash
$ ./relay -b asonix.dog blimps.xyz
```
To remove domains from the blocklist, simply pass the `-u` flag along with `-b`
```bash
$ ./relay -ub asonix.dog blimps.xyz
```
The same rules apply for allowing domains, although domains are allowed with the `-a` flag
```bash
$ ./relay -a asonix.dog blimps.xyz
$ ./relay -ua asonix.dog blimps.xyz
```

Allowed domains are only checked against incoming activities if `RESTRICTED_MODE` is enabled.
Blocks can be published in the nodeinfo metadata by setting `PUBLISH_BLOCKS` to true

For advanced setups, it may be useful to run the relay API and the background tasks in separate
processes, possibly on separate hosts. The `-j` and `-n` flags have been provided for this purpose.
By passing `-n`, a relay can be spawned that handles no deliveries. By passing `-j`, a relay will
not be spawned, but any deliveries existing in the database will be processed.

### Subscribing
Mastodon admins can subscribe to this relay by adding the `/inbox` route to their relay settings.
For example, if the server is `https://relay.my.tld`, the correct URL would be
`https://relay.my.tld/inbox`.

Pleroma admins can subscribe to this relay by adding the `/actor` route to their relay settings. For
example, if the server is `https://relay.my.tld`, the correct URL would be
`https://relay.my.tld/actor`.

### Supported Activities
- Accept Follow {remote-actor}, this is a no-op
- Reject Follow {remote-actor}, an Undo Follow is sent to {remote-actor}
- Announce {anything}, {anything} is Announced to listening servers
- Create {anything}, {anything} is Announced to listening servers
- Follow {self-actor}, become a listener of the relay, a Follow will be sent back
- Follow Public, become a listener of the relay
- Undo Follow {self-actor}, stop listening on the relay, an Undo Follow will be sent back
- Undo Follow Public, stop listening on the relay
- Delete {anything}, the Delete {anything} is relayed verbatim to listening servers
    Note that this activity will likely be rejected by the listening servers unless it has been
    signed with a JSON-LD signature
- Update {anything}, the Update {anything} is relayed verbatim to listening servers
    Note that this activity will likely be rejected by the listening servers unless it has been
    signed with a JSON-LD signature

### Supported Discovery Protocols
- Webfinger
- NodeInfo

### Configuration
By default, all these values are set to development values. These are read from the environment, or
from the `.env` file in the working directory.
```env
HOSTNAME=localhost:8080
ADDR=127.0.0.1
PORT=8080
DEBUG=true
RESTRICTED_MODE=false
VALIDATE_SIGNATURES=false
HTTPS=false
DATABASE_URL=
PRETTY_LOG=true
PUBLISH_BLOCKS=false
MAX_CONNECTIONS=4 # how many postgres connections should be made
```
To run this server in production, you'll likely want to set most of them
```env
HOSTNAME=relay.my.tld
ADDR=0.0.0.0
PORT=8080
DEBUG=false
RESTRICTED_MODE=false
VALIDATE_SIGNATURES=true
HTTPS=true
DATABASE_URL=postgres://pg_user:pg_pass@pg_host:pg_port/pg_database
PRETTY_LOG=false
PUBLISH_BLOCKS=true
MAX_CONNECTIONS=16
```

### Contributing
Unless otherwise stated, all contributions to this project will be licensed under the CSL with
the exceptions listed in the License section of this file.

### License
This work is licensed under the Cooperative Software License. This is not a Free Software
License, but may be considered a "source-available License." For most hobbyists, self-employed
developers, worker-owned companies, and cooperatives, this software can be used in most
projects so long as this software is distributed under the terms of the CSL. For more
information, see the provided LICENSE file. If none exists, the license can be found online
[here](https://lynnesbian.space/csl/). If you are a free software project and wish to use this
software under the terms of the GNU Affero General Public License, please contact me at
[asonix@asonix.dog](mailto:asonix@asonix.dog) and we can sort that out. If you wish to use this
project under any other license, especially in proprietary software, the answer is likely no.

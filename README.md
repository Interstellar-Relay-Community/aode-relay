# AodeRelay
_A simple and efficient activitypub relay_

### Usage
To simply run the server, the command is as follows
```bash
$ ./relay
```

To add domains to the blocklist, use the `-b` flag and pass a list of domains
```bash
$ ./relay -b asonix.dog blimps.xyz
```
To remove domains from the blocklist, simply pass the `-u` flag along with `-b`
```bash
$ ./relay -ub asonix.dog blimps.xyz
```
The same rules apply for whitelisting domains, although domains are whitelisted with the `-w` flag
```bash
$ ./relay -w asonix.dog blimps.xyz
$ ./relay -uw asonix.dog blimps.xyz
```

Whitelisted domains are only checked against incoming activities if `WHITELIST_MODE` is enabled

### Subscribing
Mastodon admins can subscribe to this relay by adding the `/inbox` route to their relay settings.
For example, if the server is `https://relay.my.tld`, the correct URL would be
`https://relay.my.tld/inbox`.

Pleroma admins can subscribe to this relay by adding the `/actor` route to their relay settings. For
ecample, if the server is `https://relay.my.tld`, the correct URL would be
`https://relay.my.tld/actor`.

### Supported Activities
- Announce {anything}, {anything} is Announced to listening servers
- Create {anything}, {anything} is Announced to listening servers
- Follow {self}, become a listener of the relay, a Follow will be sent back
- Follow Public, become a listener of the relay
- Undo Follow {self}, stop listening on the relay, an Undo Follow will be sent back
- Undo Follow Public, stop listening on the relay
- Delete {anything}, the Delete {anything} is relayed verbatim to listening servers
- Update {anything}, the Update {anything} is relayed verbatim to listening servers

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
WHITELIST_MODE=false
VALIDATE_SIGNATURES=false
HTTPS=false
DATABASE_URL=
```
To run this server in production, you'll likely want to set most of them
```env
HOSTNAME=relay.my.tld
ADDR=0.0.0.0
PORT=8080
DEBUG=false
WHITELIST_MODE=false
VALIDATE_SIGNATURES=true
HTTPS=true
DATABASE_URL=postgres://pg_user:pg_pass@pg_host:pg_port/pg_database
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

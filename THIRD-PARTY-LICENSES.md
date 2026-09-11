# Third-party licences

Prosperous is MIT or Apache-2.0 (see `LICENSE`). This file reproduces the licences of third-party
work that Prosperous's own source is **derived from**, as those licences require. Crates merely
depended on are not listed here - their licences travel with them in the dependency tree, and the
ones consulted as references rather than derived from are credited in `ACKNOWLEDGEMENTS.md`.

---

## Moonshine

`crates/pros-moonlight` - the Moonlight/GameStream host bridge - is **based on Moonshine** by Hans
Gaiser (<https://github.com/hgaiser/moonshine>). Moonshine is the pure-Rust GameStream host this
bridge follows: its crate choices, its module layout, and above all the exact wire behaviour of the
RTSP handshake, the RTP/NV video packetisation, the Reed-Solomon FEC scheme and the AES-GCM control
channel are adapted from it. Where a Prosperous source file is a close adaptation of a Moonshine
one, it says so in its module documentation and points here.

Moonshine is distributed under the BSD 2-Clause Licence, reproduced in full below. That licence
permits this use and requires that the copyright notice, conditions and disclaimer be retained,
which is the purpose of this section.

```
BSD 2-Clause License

Copyright (c) 2024, Hans Gaiser

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

#!/bin/sh
# Builds on Ubuntu 22.04 against Ubuntu 24.04's PipeWire 1.0.5 headers while linking against
# the system's older libpipewire (0.3.48), whose headers the pipewire crate no longer compiles
# against. Linking against the old library keeps the build honest: a function 0.3.48 lacks
# fails the link instead of the agent's start. See docs/conventions/desktop.md, "PipeWire on
# Ubuntu 22.04".
#
# Usage: pipewire-headers.sh <folder> [libdir]
# Prints the pkg-config folder to put first in PKG_CONFIG_PATH.
set -eu

dir=$1
libdir=${2:-/usr/lib/x86_64-linux-gnu}
base=http://archive.ubuntu.com/ubuntu/pool/main/p/pipewire

mkdir -p "$dir/debs" "$dir/root" "$dir/pkgconfig"
while read -r package sha256; do
  curl -fsSL --retry 3 -o "$dir/debs/$package" "$base/$package"
  echo "$sha256  $dir/debs/$package" | sha256sum -c - >&2
  dpkg-deb -x "$dir/debs/$package" "$dir/root"
done <<'PACKAGES'
libpipewire-0.3-dev_1.0.5-1_amd64.deb c0b34ed6a34391a94c0711966dbea9b6b5a98758d88813824c05d6662e0b60c9
libspa-0.2-dev_1.0.5-1_amd64.deb 6c99d162e23e13cacf8bc3edc312c60545281078398e52e34dc5ebb4209649dc
PACKAGES

for pc in libpipewire-0.3 libspa-0.2; do
  sed -e "s|^prefix=.*|prefix=$dir/root/usr|" -e "s|^libdir=.*|libdir=$libdir|" \
    "$dir/root/usr/lib/x86_64-linux-gnu/pkgconfig/$pc.pc" > "$dir/pkgconfig/$pc.pc"
done
echo "$dir/pkgconfig"

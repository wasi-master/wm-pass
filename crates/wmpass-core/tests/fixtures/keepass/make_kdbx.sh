#!/bin/sh
# Writes the KeePassXC fixtures beside this script with the real keepassxc-cli, so the
# tests check WM Pass against files KeePassXC itself made. Needs keepassxc-cli 2.7 or
# later and a built `wmpass` CLI (cargo build -p wmpass-cli). Each run makes new random
# seeds, so only rerun it to change the fixtures.
set -eu
cd "$(dirname "$0")"
root=$(cd ../../../../.. && pwd)
wmpass="$root/target/debug/wmpass"
rm -f kpxc-*.kdbx kpxc.keyx key-*.key key-*.bin

# keepassxc-cli writes KDBX 3.1 with AES-KDF. These hold codes in three apps' forms,
# field references, history, tags, a protected field and a recycle bin.
printf 'xc-pw\nxc-pw\n' | keepassxc-cli import -q -p -t 100 kpxc-source.xml kpxc-kdbx31-password.kdbx
printf 'xc-pw\nxc-pw\n' | keepassxc-cli import -q -p -t 100 --set-key-file kpxc.keyx kpxc-source.xml kpxc-kdbx31-password-and-key-file.kdbx
keepassxc-cli import -q -t 100 --set-key-file kpxc.keyx kpxc-source.xml kpxc-kdbx31-key-file-only.kdbx

# KDBX 4.0 with Argon2id, written by KeePassXC: the same content merged into a KDBX 4
# shell, which KeePassXC then saves in its own way.
tmp=$(mktemp -d)
WMPASS_STATE_DIR="$tmp/state" WMPASS_PASSWORD=xc-pw "$wmpass" create "$tmp/v.wmvault" >/dev/null
WMPASS_STATE_DIR="$tmp/state" WMPASS_PASSWORD=xc-pw "$wmpass" export "$tmp/v.wmvault" kdbx kpxc-kdbx40-password.kdbx >/dev/null
printf 'xc-pw\n' | keepassxc-cli merge -q -s kpxc-kdbx40-password.kdbx kpxc-kdbx31-password.kdbx
rm -rf "$tmp"

# The older key file forms KeePassXC still reads, each locking a database alone.
printf '%s' 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f > key-hex64.key
printf '\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020\021\022\023\024\025\026\027\030\031\032\033\034\035\036\037\040' > key-binary32.key
head -c 700 kpxc-source.xml > key-arbitrary.bin
cat > key-xml-v1.key <<'EOF'
<?xml version="1.0" encoding="utf-8"?>
<KeyFile>
	<Meta>
		<Version>1.00</Version>
	</Meta>
	<Key>
		<Data>AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=</Data>
	</Key>
</KeyFile>
EOF
for key in key-hex64.key key-binary32.key key-arbitrary.bin key-xml-v1.key; do
  keepassxc-cli db-create -q -t 100 --set-key-file "$key" "kpxc-key-file-${key%.*}.kdbx"
  keepassxc-cli add -q --no-password -k "$key" -u "$key" "kpxc-key-file-${key%.*}.kdbx" Entry
done

# A plain KDBX 3.1 database made and edited with keepassxc-cli alone.
printf 'xc-pw\nxc-pw\n' | keepassxc-cli db-create -q -p -t 100 kpxc-kdbx31-made-by-cli.kdbx
printf 'xc-pw\n' | keepassxc-cli mkdir -q kpxc-kdbx31-made-by-cli.kdbx Team
printf 'xc-pw\nfirst-pass\n' | keepassxc-cli add -q -u me --url https://t.example -p kpxc-kdbx31-made-by-cli.kdbx Team/Mail

#!/usr/bin/env bash
set -euo pipefail
umask 077

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
certificate_directory="$(python3 "$repo_root/scripts/dev/runtime.py" certificate-directory)"
certificate_names=(ca server client client-device-2 untrusted-ca untrusted-client)

validate_certificate_bundle() {
  for certificate_name in "${certificate_names[@]}"; do
    if [[ ! -f "$certificate_directory/$certificate_name.crt" || ! -f "$certificate_directory/$certificate_name.key" ]]; then
      echo "Incomplete synthetic certificate bundle. Stop the environment and run the explicit synthetic reset." >&2
      return 1
    fi
    chmod 600 "$certificate_directory/$certificate_name.crt" "$certificate_directory/$certificate_name.key"
    if ! openssl x509 -checkend 0 -noout -in "$certificate_directory/$certificate_name.crt" >/dev/null 2>&1; then
      echo "Synthetic certificate expired or is invalid. Stop the environment and run the explicit synthetic reset." >&2
      return 1
    fi
    openssl pkey -check -noout -in "$certificate_directory/$certificate_name.key" >/dev/null 2>&1
    certificate_public_key="$(openssl x509 -pubkey -noout -in "$certificate_directory/$certificate_name.crt")"
    private_public_key="$(openssl pkey -pubout -in "$certificate_directory/$certificate_name.key")"
    if [[ "$certificate_public_key" != "$private_public_key" ]]; then
      echo "Synthetic certificate does not match its private key. Stop the environment and run the explicit synthetic reset." >&2
      return 1
    fi
  done
  openssl verify -CAfile "$certificate_directory/ca.crt" -purpose sslserver "$certificate_directory/server.crt" >/dev/null 2>&1
  openssl verify -CAfile "$certificate_directory/ca.crt" -purpose sslclient "$certificate_directory/client.crt" "$certificate_directory/client-device-2.crt" >/dev/null 2>&1
  openssl verify -CAfile "$certificate_directory/untrusted-ca.crt" -purpose sslclient "$certificate_directory/untrusted-client.crt" >/dev/null 2>&1
  if openssl verify -CAfile "$certificate_directory/ca.crt" "$certificate_directory/untrusted-client.crt" >/dev/null 2>&1; then
    echo "Untrusted synthetic test client unexpectedly chains to the trusted test CA." >&2
    return 1
  fi
}

if [[ -n "$(find "$certificate_directory" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  validate_certificate_bundle
  echo "Existing private synthetic certificates are valid."
  exit 0
fi

temporary_directory="$(mktemp -d "$certificate_directory/.generating.XXXXXX")"
trap 'rm -rf "$temporary_directory"' EXIT

generate_root() {
  local certificate_name="$1"
  local common_name="$2"
  openssl req -new -x509 -newkey rsa:2048 -nodes -sha256 -days 7 \
    -subj "/CN=$common_name/O=ChargeShare Synthetic Only" \
    -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -keyout "$temporary_directory/$certificate_name.key" \
    -out "$temporary_directory/$certificate_name.crt" >/dev/null 2>&1
}

generate_leaf() {
  local certificate_name="$1"
  local common_name="$2"
  local certificate_authority="$3"
  local serial_number="$4"
  local usage="$5"
  openssl req -new -newkey rsa:2048 -nodes -sha256 \
    -subj "/CN=$common_name/O=ChargeShare Synthetic Only/OU=Synthetic Local Fixtures" \
    -keyout "$temporary_directory/$certificate_name.key" \
    -out "$temporary_directory/$certificate_name.csr" >/dev/null 2>&1
  printf '%s\n' \
    "basicConstraints=critical,CA:FALSE" \
    "keyUsage=critical,digitalSignature,keyEncipherment" \
    "extendedKeyUsage=$usage" > "$temporary_directory/$certificate_name.extensions"
  if [[ "$usage" == "serverAuth" ]]; then
    printf '%s\n' "subjectAltName=DNS:receiver,DNS:localhost,IP:127.0.0.1" >> "$temporary_directory/$certificate_name.extensions"
  fi
  openssl x509 -req -sha256 -days 7 \
    -in "$temporary_directory/$certificate_name.csr" \
    -CA "$temporary_directory/$certificate_authority.crt" \
    -CAkey "$temporary_directory/$certificate_authority.key" \
    -set_serial "$serial_number" \
    -extfile "$temporary_directory/$certificate_name.extensions" \
    -out "$temporary_directory/$certificate_name.crt" >/dev/null 2>&1
}

generate_root ca "Tesla Motors Products CA"
generate_root untrusted-ca "ChargeShare Untrusted Synthetic Test CA"
generate_leaf server receiver ca 1 serverAuth
generate_leaf client device-1 ca 2 clientAuth
generate_leaf client-device-2 device-2 ca 3 clientAuth
generate_leaf untrusted-client device-1 untrusted-ca 1 clientAuth

for certificate_name in "${certificate_names[@]}"; do
  chmod 600 "$temporary_directory/$certificate_name.crt" "$temporary_directory/$certificate_name.key"
  mv "$temporary_directory/$certificate_name.crt" "$certificate_directory/$certificate_name.crt"
  mv "$temporary_directory/$certificate_name.key" "$certificate_directory/$certificate_name.key"
done

validate_certificate_bundle
echo "Generated seven-day synthetic test certificates in the private project runtime directory."

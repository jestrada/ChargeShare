{
  description = "Pinned Linux x86_64 tools for ChargeShare synthetic local development";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/a7868a727837f3c09cee2ce0ca671c76b1589fed";
    rust-overlay = {
      url = "github:oxalica/rust-overlay/dbc715a4b7c0ace63b9769a032d1dd34cd89e5bd";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      projectTilt = pkgs.writeShellScriptBin "tilt" ''
        set -euo pipefail

        if [[ "$#" == 1 && ( "$1" == --help || "$1" == -h || "$1" == --version ) ]]; then
          exec ${pkgs.tilt}/bin/tilt "$@"
        fi
        tilt_command=""
        skip_argument=false
        for tilt_argument in "$@"; do
          if [[ "$skip_argument" == true ]]; then
            skip_argument=false
            continue
          fi
          case "$tilt_argument" in
            --klog)
              skip_argument=true
              ;;
            --klog=*|-d|--debug|--debug=*|-d=*|-v|--verbose|--verbose=*|-v=*|--help=*|-h=*|--version=*)
              ;;
            --)
              ;;
            *)
              if [[ "$tilt_argument" =~ ^-[dv]+$ ]]; then
                continue
              fi
              if [[ "$tilt_argument" == -* ]]; then
                printf 'Unsupported Tilt option before the command. Put up, ci or down first, followed by reviewed lifecycle options.\n' >&2
                exit 1
              fi
              tilt_command="$tilt_argument"
              break
              ;;
          esac
        done

        case "$tilt_command" in
          up|ci|down)
            if [[ ! -f scripts/dev/tilt.sh ]]; then
              printf 'Run Tilt lifecycle commands from the ChargeShare repository root.\n' >&2
              exit 1
            fi
            exec ${pkgs.bash}/bin/bash scripts/dev/tilt.sh ${pkgs.tilt}/bin/tilt "$@"
            ;;
          *)
            exec ${pkgs.tilt}/bin/tilt "$@"
            ;;
        esac
      '';
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        name = "chargeshare-local-development";
        packages = [
          rustToolchain
          projectTilt
          pkgs.docker-client
          pkgs.docker-compose
          pkgs.nodejs_24
          pkgs.go_1_27
          pkgs.bashInteractive
          pkgs.openssl
          pkgs.pkg-config
          pkgs.jq
          pkgs.curl
          pkgs.python3
          pkgs.git
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnused
          pkgs.gawk
          pkgs.gnutar
          pkgs.gzip
          pkgs.procps
          pkgs.iproute2
          pkgs.util-linux
        ];
        buildInputs = [ pkgs.openssl.dev ];
      };
    };
}

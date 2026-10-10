FROM docker.io/library/golang:1.27.1-bookworm@sha256:966278043a40889499db9b0cd196fc789c37c385d41bd9a10cb1e7764af60cdc AS receiver-build

WORKDIR /build
ADD --checksum=sha256:018d79fe0a045cca07331d37bd0cb57b2e838c51bc48fd837a1472e50068bbea https://github.com/jedisct1/libsodium/releases/download/1.0.19-RELEASE/libsodium-1.0.19.tar.gz /build/libsodium.tar.gz
RUN tar -xzf /build/libsodium.tar.gz -C /build && cd /build/libsodium-stable && ./configure --disable-shared --enable-static && make -j2 && make install

ADD --checksum=sha256:c593001a89f5a85dd2ddf564805deb860e02471171b3f204944857336295c3e5 https://github.com/zeromq/libzmq/releases/download/v4.3.4/zeromq-4.3.4.tar.gz /build/zeromq.tar.gz
RUN tar -xzf /build/zeromq.tar.gz -C /build && cd /build/zeromq-4.3.4 && ./configure --enable-static --disable-shared --disable-Werror && make -j2 && make install

ADD --checksum=sha256:cb5226fe19f55e2d112244463668d9769b3bf9d65d30805258e3b29bfcdf140f https://codeload.github.com/teslamotors/fleet-telemetry/tar.gz/bd076fe1494841707528449560c4a19d0d426da4 /build/fleet-telemetry.tar.gz
RUN mkdir /build/fleet-telemetry && tar -xzf /build/fleet-telemetry.tar.gz -C /build/fleet-telemetry --strip-components=1

WORKDIR /build/fleet-telemetry
ENV GOTOOLCHAIN=local CGO_ENABLED=1 CGO_LDFLAGS="-lstdc++"
RUN go mod download && go mod verify && go build -mod=readonly --ldflags 'extldflags="-static"' -o /fleet-telemetry cmd/main.go

FROM docker.io/library/golang:1.27.1-bookworm@sha256:966278043a40889499db9b0cd196fc789c37c385d41bd9a10cb1e7764af60cdc
WORKDIR /
COPY --from=receiver-build /fleet-telemetry /fleet-telemetry
COPY --from=receiver-build /build/fleet-telemetry/LICENSE /usr/share/doc/fleet-telemetry/LICENSE
COPY --from=receiver-build /build/libsodium-stable/LICENSE /usr/share/doc/libsodium/LICENSE
COPY --from=receiver-build /build/zeromq-4.3.4/COPYING /usr/share/doc/libzmq/COPYING
COPY --from=receiver-build /build/zeromq-4.3.4/COPYING.LESSER /usr/share/doc/libzmq/COPYING.LESSER
COPY --from=receiver-build /build/zeromq-4.3.4/AUTHORS /usr/share/doc/libzmq/AUTHORS
ENTRYPOINT ["/fleet-telemetry"]

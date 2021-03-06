# Copyright 2019 Cargill Incorporated
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

FROM hyperledger/grid-dev as grid-schema-builder

COPY contracts/schema /build/contracts/schema
COPY sdk/ /build/sdk/

# Do a release build to cache dependencies
WORKDIR /build/contracts/schema

# Build the contract
ARG REPO_VERSION
RUN sed -i -e "0,/version.*$/ s/version.*$/version\ =\ \"${REPO_VERSION}\"/" Cargo.toml
RUN cargo build --target wasm32-unknown-unknown --release

# Build a scar file
# Copy the packaging directory
COPY contracts/schema/packaging/scar/* \
     /build/contracts/schema/packaging/scar/

# Copy the contract to the packaging directory
RUN cp /build/target/wasm32-unknown-unknown/release/grid-schema-tp.wasm \
    packaging/scar

WORKDIR /build/contracts/schema/packaging/scar

# Create .scar file
RUN tar -jcvf /tmp/grid-schema${REPO_VERSION}.scar .

# -------------=== schema submitter build ===-------------

FROM hyperledger/sawtooth-sabre-cli

COPY --from=grid-schema-builder \
     /build/target/wasm32-unknown-unknown/release/grid-schema-tp.wasm /tmp

# Copy the contract definition
COPY contracts/schema/schema.yaml /tmp

COPY --from=grid-schema-builder \
    /tmp/grid-schema*.scar /tmp

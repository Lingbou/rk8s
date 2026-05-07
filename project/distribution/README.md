# Distribution

A lite standalone implementation of the OCI Distribution Spec in Rust. This fork runs without registry authentication and treats all clients as the `admin` user.

## Configuration

The registry is configured via environment variables, which can be loaded from a `.env` file or command-line arguments. The application intelligently adapts its database connection based on the variables provided.

### For Local Development (using `cargo run`)

For local development, the easiest method is to provide a complete `DATABASE_URL` connection string. The application will detect and use this variable directly.

Create a `.env` file in the project's root directory:

```dotenv
# Application Host and Port
OCI_REGISTRY_URL=127.0.0.1
OCI_REGISTRY_PORT=8968

# Public URL used in responses
OCI_REGISTRY_PUBLIC_URL=http://127.0.0.1:8968

# Storage Configuration
OCI_REGISTRY_STORAGE=FILESYSTEM
OCI_REGISTRY_ROOTDIR=/var/lib/registry

# --- Database Configuration (Direct Method) ---
# Provide the full URL for local development
DATABASE_URL="postgres://postgres:password@localhost:5432/postgres"

# A secret key for signing JWT tokens.
# Generate a secure random string for production use.
JWT_SECRET="secret"

# JWT token lifetime in seconds
JWT_LIFETIME_SECONDS=3600

# Default user injected for compatibility token responses
OCI_REGISTRY_DEFAULT_USER=admin

# Log level
RUST_LOG="info"
```

### For Docker Compose (Recommended)

For the Docker Compose environment, you should provide the database connection components separately. The application will detect that `DATABASE_URL` is not set and will construct the correct connection string for the container network itself.

Create a `.env` file with the following content:

```dotenv
# ===============================================
# Docker Compose Orchestration Config
# ===============================================
APP_PORT=8968
DB_EXTERNAL_PORT=5433
POSTGRES_VERSION=15

# ===============================================
# Application Runtime Config
# ===============================================
# Bind to 0.0.0.0 to accept connections from outside the container
OCI_REGISTRY_URL=0.0.0.0
OCI_REGISTRY_PORT=8968

# Public URL accessible by clients
OCI_REGISTRY_PUBLIC_URL=http://127.0.0.1:8968

# Storage path inside the container
OCI_REGISTRY_STORAGE=FILESYSTEM
OCI_REGISTRY_ROOTDIR=/var/lib/oci-registry

# --- Database Configuration (Component Method) ---
# DO NOT set DATABASE_URL here. Provide components instead.
POSTGRES_HOST=db
POSTGRES_PORT=5432
POSTGRES_USER=postgres
POSTGRES_PASSWORD=password
POSTGRES_DB=postgres

# --- Security Configuration ---
JWT_SECRET="secret"
JWT_LIFETIME_SECONDS=3600
OCI_REGISTRY_DEFAULT_USER=admin

# Log level
RUST_LOG="info"
```

**Security Note**: This standalone build has no registry authentication. Deploy it only on a trusted single-machine or private-network environment.

## Quick Start

### With Cargo (Local Development)

1.  **Prerequisites**: Ensure you have a PostgreSQL server running and accessible.
2.  **Configure**: Create a `.env` file for local development as described above (using `DATABASE_URL`).
3.  **Start**: Run the application using Cargo.
    ```bash
    cargo run
    ```
The registry will now be running and listening on `127.0.0.1:8968`.

### With Docker Compose (Recommended)

This is the easiest way to get started, as it manages both the application and its database.

1.  **Prerequisites**: Docker and Docker Compose must be installed.
2.  **Configure**: Create a `.env` file for Docker Compose as described above (using separate `POSTGRES_*` variables).
3.  **Start**: Use Docker Compose to build and start the services.
    ```bash
    docker-compose up --build -d
    ```
    *   `--build`: Forces a rebuild of the application image if you've made code changes.
    *   `-d`: Runs the containers in detached mode.

4.  **Check Status**: You can check if the services are running correctly.
    ```bash
    docker-compose ps
    ```

5.  **View Logs**: To see the application logs in real-time:
    ```bash
    docker-compose logs -f distribution
    ```
The registry will be running and accessible on `http://127.0.0.1:8968`.

6.  **Stopping**: To stop and remove the containers:
    ```bash
    docker-compose down
    ```

## User and Repository Management

This registry extends the OCI specification with repository metadata APIs. Authentication is disabled.

### 1. Compatibility Token

*   **Endpoint**: `GET /auth/token`
*   **Authentication**: None. Basic Auth, if sent by a client, is ignored.
*   **Example using curl**:
    ```bash
    curl "http://127.0.0.1:8968/auth/token"
    ```
*   **Response**: A JSON object containing the JWT.
    ```json
    {
      "token": "ey...",
      "access_token": "ey...",
      "expires_in": 3600,
      "issued_at": "2025-09-17T..."
    }
    ```
The token is provided only for Docker/OCI client compatibility. Registry endpoints also work without it.

### 2. Repository Management

#### List Visible Repositories

List all repositories:

*   **Endpoint**: `GET /api/v1/repo`
*   **Authentication**: None
*   **Response**: 
    ```json
    {
      "data": [
        {
          "namespace": "admin",
          "name": "myrepo",
          "is_public": true,
          "tags": ["latest", "v1"],
          "size_tag": "latest",
          "size_bytes": 123456,
          "last_pushed_at": "2026-04-13T08:30:00Z"
        }
      ]
    }
    ```

#### Change Repository Visibility

Repositories can still store a `public`/`private` flag for UI metadata, but the flag is not used for access control in this build.

*   **Endpoint**: `PUT /api/v1/<namespace>/<repo>/visibility`
*   **Authentication**: None
*   **Request Body**:
    ```json
    {
        "visibility": "private"
    }
    ```   
*   **Response**: `200 OK` on success.
*   **Note**: The `visibility` field can be either `"public"` or `"private"`.

## Command-Line Options

While using a `.env` file is recommended, configuration can be overridden via command-line arguments. Based on the new configuration logic, the database can be configured with component flags.

```
Usage: distribution [OPTIONS]

Options:
      --host <HOST>        Registry listening host [env: OCI_REGISTRY_URL] [default: 127.0.0.1]
  -p, --port <PORT>        Registry listening port [env: OCI_REGISTRY_PORT] [default: 8968]
  -s, --storage <STORAGE>  Storage backend type [env: OCI_REGISTRY_STORAGE] [default: FILESYSTEM]
      --root <ROOT>        Registry root path [env: OCI_REGISTRY_ROOTDIR] [default: /var/lib/registry]
      --url <URL>          Registry url [env: OCI_REGISTRY_PUBLIC_URL] [default: http://127.0.0.1:8968]
      --db-host <DB_HOST>  Database host [env: POSTGRES_HOST] [default: localhost]
      --db-port <DB_PORT>  Database port [env: POSTGRES_PORT] [default: 5432]
      --db-user <DB_USER>  Database user [env: POSTGRES_USER] [default: postgres]
      --db-name <DB_NAME>  Database name [env: POSTGRES_DB] [default: postgres]
  -h, --help               Print help
  -V, --version            Print version
```
**Note**: The database password is intentionally not exposed as a command-line argument for security reasons. It must be provided via the `POSTGRES_PASSWORD` environment variable if `DATABASE_URL` is not set.

## Build from source

Build with Buck2 (make sure you have followed the workflow in `third-party/README.md` before your build):

```
buck2 build //project/distribution:distribution
```

Another option is to build with Cargo:

```
cd project/distribution/
cargo build
```

## Compatibility

The distribution registry implements the [OCI Distribution Spec](https://github.com/opencontainers/distribution-spec) version 1.1.1.

| ID      | Method         | API Endpoint                                                 | Compatibility |
| ------- | -------------- | ------------------------------------------------------------ | ------------- |
| end-1   | `GET`          | `/v2/`                                                       | ✅             |
| end-2   | `GET` / `HEAD` | `/v2/<name>/blobs/<digest>`                                  | ✅             |
| end-3   | `GET` / `HEAD` | `/v2/<name>/manifests/<reference>`                           | ✅             |
| end-4a  | `POST`         | `/v2/<name>/blobs/uploads/`                                  | ✅             |
| end-4b  | `POST`         | `/v2/<name>/blobs/uploads/?digest=<digest>`                  | ✅             |
| end-5   | `PATCH`        | `/v2/<name>/blobs/uploads/<reference>`                       | ✅             |
| end-6   | `PUT`          | `/v2/<name>/blobs/uploads/<reference>?digest=<digest>`       | ✅             |
| end-7   | `PUT`          | `/v2/<name>/manifests/<reference>`                           | ✅             |
| end-8a  | `GET`          | `/v2/<name>/tags/list`                                       | ✅             |
| end-8b  | `GET`          | `/v2/<name>/tags/list?n=<integer>&last=<tagname>`            | ✅             |
| end-9   | `DELETE`       | `/v2/<name>/manifests/<reference>`                           | ✅             |
| end-10  | `DELETE`       | `/v2/<name>/blobs/<digest>`                                  | ✅             |
| end-11  | `POST`         | `/v2/<name>/blobs/uploads/?mount=<digest>&from=<other_name>` | 🚧             |
| end-12a | `GET`          | `/v2/<name>/referrers/<digest>`                              | 🚧             |
| end-12b | `GET`          | `/v2/<name>/referrers/<digest>?artifactType=<artifactType>`  | 🚧             |
| end-13  | `GET`          | `/v2/<name>/blobs/uploads/<reference>`                       | ✅             |

## Integration Tests

The project includes integration tests that run inside a QEMU/KVM virtual machine using the [qlean](https://crates.io/crates/qlean) crate. These tests verify user permissions and repository access controls.

### Prerequisites

Before running the integration tests, ensure you have:

1. **QEMU/KVM** installed and configured on your Linux host
2. Required tools: `qemu-system-x86_64`, `qemu-img`, `guestfish`, `virt-copy-out`, `xorriso`

You can verify the installation with:

```bash
qemu-system-x86_64 --version
qemu-img --version
guestfish --version
```

### Running the Tests

1. First, build the distribution binary (debug mode required):

```bash
cd project
cargo build -p distribution
```

2. Run the integration tests:

```bash
cd project
RUST_LOG=info cargo test -p distribution --test test_registry_integration -- --nocapture
```

The tests will:
- Create a Debian VM
- Install and configure PostgreSQL
- Upload and start the distribution service
- Verify no-auth blob upload and repository metadata behavior for the default `admin` namespace

**Note**: The first run may take longer as it downloads the VM image. Subsequent runs will be faster.

## Conformance Tests

To run the conformance tests provided by OCI Distribution Spec, you need to install Go 1.17+ first, and then clone the distribution-spec repository:

```bash
git clone git@github.com:opencontainers/distribution-spec.git
```

In the `conformance` directory, apply a patch and build the test binary:

```bash
cd distribution-spec/conformance/
go test -c
```

This will produce an executable at `conformance.test`.

Next, set environment variables with the registry details. **Note**: Before running the tests, you must create a user via the API as described in the User Management section.

```bash
# Registry details
export OCI_ROOT_URL="http://127.0.0.1:8968"
export OCI_NAMESPACE="myorg/myrepo"
export OCI_CROSSMOUNT_NAMESPACE="myorg/other"

# Credentials for the user you created
export OCI_USERNAME="myuser"
export OCI_PASSWORD="mypass"

# Which workflows to run
export OCI_TEST_PULL=1
export OCI_TEST_PUSH=1
export OCI_TEST_CONTENT_DISCOVERY=1
export OCI_TEST_CONTENT_MANAGEMENT=1

# Extra settings
export OCI_HIDE_SKIPPED_WORKFLOWS=0
export OCI_DEBUG=0
export OCI_DELETE_MANIFEST_BEFORE_BLOBS=0 # defaults to OCI_DELETE_MANIFEST_BEFORE_BLOBS=1 if not set
```

Lastly, run the tests:

```bash
./conformance.test
```

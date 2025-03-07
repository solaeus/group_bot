# Veloren Group Bot

Group management bot for [Veloren](https://veloren.net)

## Usage

If you choose to run this but with Docker or Podman, you only need to create a local secrets.toml.
But if you choose to compile you must clone this repository.

### Step 1: Create config.toml

Create a secrets.toml file in the project root.

```toml
# config.toml
username = "YOUR_USERNAME"
password = "YOUR_PASSWORD"
character = "YOUR_CHARACTER"
admin_list = []
```

### Step 2: Run via Docker/Podman

Use Podman (or Docker) to run the release build without exposing secrets. First, create the secret.

```sh
podman secret create secrets.toml secrets.toml
```

Then run the container.

```sh
podman run \
    --detach \
    --secret secrets.toml \
    --env SECRETS=/run/secrets/secrets.toml \
    git.jeffa.io/jeff/group_bot
```

### Step 2 (Alternate): Run via cargo

Install [rustup](https://rustup.sh) and use cargo to compile and run the bot.

```sh
SECRETS=secrets.toml cargo run --release
```

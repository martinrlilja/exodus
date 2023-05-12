# Exodus

Extract OTP secrets from Google Authenticator exports.

This helps you migrate away from Google Authenticator to 1Password or other authenticator apps. 

## Development

[Install trunk](https://github.com/thedodd/trunk)

```sh
$ trunk serve
```

You might need the latest version of trunk from git to fix some issues,
install through cargo like this:

```sh
$ cargo install -f --git https://github.com/thedodd/trunk.git trunk
```

## Build

```sh
$ trunk build --release
```

Upload the contents of the ./dist/ folder to a web server.

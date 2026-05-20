# 4STM4 Mobile OS — Developer SDK

## Overview

The SDK lets third-party apps communicate with Mobile OS daemons
via Unix sockets using the line-delimited JSON protocol.

## Sockets

| Socket | Group | Purpose |
|--------|-------|---------|
| `/run/commd/ui.sock` | `comm-ui` | Messages, conversations, send |
| `/run/commd/admin.sock` | `comm-admin` | Administrative actions |
| `/run/simd.sock` | `dialout` | Telephony (call, SMS) |
| `/run/netd.sock` | root | Network management |
| `/run/powerd.sock` | root | Power management |
| `/run/hardwared.sock` | root | Hardware profile |

## Envelope format

```json
{
  "version": 1,
  "type": "REQUEST",
  "request_id": "my-unique-id",
  "ts_ms": 1716220000000,
  "source": "myapp",
  "action": "LIST_CONVERSATIONS",
  "body": {}
}
```

Response `type` is `RESPONSE`; errors are `ERROR` type.

## commd UI actions

### `LIST_CONVERSATIONS`
Returns 50 most-recent conversations.

### `GET_MESSAGES`
```json
{ "conv_id": "tg-123456789", "limit": 50, "before_ts": 1716220000 }
```

### `SEND`
```json
{ "conv_id": "tg-123456789", "backend": "telegram", "text": "Hello" }
```

### `MARK_READ`
```json
{ "conv_id": "tg-123456789" }
```

### `SAVE_DRAFT`
```json
{ "conv_id": "tg-123456789", "body": "draft text" }
```

### `SUBSCRIBE`
Parks the connection for push events. Events arrive as JSON lines:
```json
{ "type": "EVENT", "action": "NEW_MESSAGE", "body": { ... } }
```

## simd (telephony)

Plain text protocol on `/run/simd.sock`:
- `STATUS` → JSON status
- `DIAL <number>` → `OK` or `ERROR`
- `HANGUP` → `OK`
- `ANSWER` → `OK`
- `SEND_SMS <number> <message>` → `OK`

## powerd

Plain text or JSON on `/run/powerd.sock`:
- `STATUS` → JSON {bat_pct, bat_charging, screen, brightness, ...}
- `ACTIVITY` → resets idle timer, turns screen on
- `SUBSCRIBE` → push events (SCREEN_DIM, SCREEN_OFF, ALERT ...)
- `SET_BRIGHTNESS <pct>` → 5–100
- `SET_DIM_SECS <s>` / `SET_OFF_SECS <s>`
- `INHIBIT_SLEEP` / `ALLOW_SLEEP`

## Building packages

Add your package to the external tree:
```
packages/mypkg/Config.in
packages/mypkg/mypkg.mk
packages/mypkg/src/...
```

Rebuild with `make build`.

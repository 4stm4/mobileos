#!/usr/bin/env python3
"""Start mobileos configurator web UI."""
import uvicorn

if __name__ == "__main__":
    uvicorn.run("app:app", host="0.0.0.0", port=8765, reload=False)

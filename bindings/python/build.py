import os
import platform
import subprocess
import shutil
import sys
from pathlib import Path

# Fallback to local target dir if env var is not set
CARGO_TARGET_DIR = os.environ.get("CARGO_TARGET_DIR", "../../target")

FEATURES = ""

# Determine correct dynamic library extension for current OS
SYSTEM = platform.system()
LIB_NAME = "moonlight_common_bindings"

if SYSTEM == "Windows":
    LIB_FILE = f"{LIB_NAME}.dll"
elif SYSTEM == "Darwin":
    raise "no mac for you"
else:
    LIB_FILE = f"lib{LIB_NAME}.so"

LIB_OUTPUT_PATH = Path(CARGO_TARGET_DIR) / "debug" / LIB_FILE

try:
    # Build the Rust project
    subprocess.run(["cargo",  "build", "-p", "moonlight-common-bindings", "--features", FEATURES], check=True)

    # Copy the output library
    shutil.copy2(LIB_OUTPUT_PATH, f"./{LIB_FILE}")

    # Generate UniFFI Python bindings
    subprocess.run(
        [
            "cargo",
            "run",
            "-p",
            "uniffi-bindgen",
            "generate",
            str(LIB_OUTPUT_PATH),
            "--language",
            "python",
            "--out-dir",
            "./",
        ],
        check=True,
    )
except subprocess.CalledProcessError as e:
    print(f"\nCommand failed with exit code {e.returncode}")
    sys.exit(e.returncode)
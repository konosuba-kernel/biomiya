"""Run independent F/M probes from the biomiya repository root."""
import json
import subprocess

build = subprocess.run([
    "cargo", "build", "--manifest-path", "core-s/Cargo.toml", "--features",
    "frontend,check,backend,image", "--lib", "--message-format", "json"
], text=True, capture_output=True, check=True)
messages = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
rlib = next(name for item in messages
            if item.get("reason") == "compiler-artifact" and item["target"]["name"] == "biomiya_core_s"
            for name in item["filenames"] if name.endswith(".rlib"))
subprocess.run([
    "rustc", "--edition", "2024", "--test", "docs/workflows/bootstrap-b2/probes/fm.rs",
    "--extern", "biomiya_core_s=" + rlib, "-L", "dependency=core-s/target/debug/deps",
    "-o", "/tmp/konosuba-b2-fm-review"
], check=True)
subprocess.run(["/tmp/konosuba-b2-fm-review", "--nocapture"], check=True)

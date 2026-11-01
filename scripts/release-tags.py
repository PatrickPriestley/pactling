#!/usr/bin/env python3
"""Image tags for a release already checked against Cargo.toml by packaging."""
import sys

tag = sys.argv[1]
image = "ghcr.io/patrickpriestley/pactling"
print(f"{image}:{tag}")
if "-" not in tag:
    print(f"{image}:latest")

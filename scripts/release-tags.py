#!/usr/bin/env python3
"""GitHub step outputs for a tag already checked against Cargo.toml by packaging."""
import sys

tag = sys.argv[1]
image = "ghcr.io/patrickpriestley/pactling"
print("value<<PACTLING_TAGS")
print(f"{image}:{tag}")
if "-" not in tag:
    print(f"{image}:latest")
print("PACTLING_TAGS")
print(f"prerelease={str('-' in tag).lower()}")

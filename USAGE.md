# Podoxide Usage Guide

This guide documents the command-line syntax, configuration schema, recipe definitions, targeting rules, and modes of Podoxide (`pods`).

## Table of Contents

- [Command Syntax](#command-syntax)
- [Configuration Reference](#configuration-reference)
- [Recipe Reference](#recipe-reference)

## Command Syntax

Invoke Podoxide with the `pods` binary:

```bash
pods [MODE] [OPTIONS] [ACTION] [TARGETS...]
```

## Configuration Reference

By default, Podoxide loads `./pods.conf`. See [`pods.conf`](pods.conf) for the root template.

## Recipe Reference

Recipes define Podman pods and containers using the `.pod` block-based DSL. See [`recipe.pod`](recipe.pod) for the root template.

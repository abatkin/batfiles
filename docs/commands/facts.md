# Host facts

## `facts list`

```text
batfiles facts list
```

Print the [host facts](../environment.md#host-facts) that conditions and
templates read as `facts.<key>`, one per line, on standard output:

```text
arch     = "x86_64"
family   = "unix"
hostname = "silver.example.net"
os       = "linux"
```

Keys are sorted and aligned, and values are quoted as
[`vars list`](vars.md#vars-list) quotes them. The command reads no repository
and no state, so it works on a machine without a leaf repository. To see what a
template built from these facts resolves to, use
[`vars eval`](vars.md#vars-eval).

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).

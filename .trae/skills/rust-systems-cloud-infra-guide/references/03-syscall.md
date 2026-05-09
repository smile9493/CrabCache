# Skill: Kernel Interaction & Syscall Safety

## 👤 Profile

* **Domain**: eBPF control plane, custom storage engines, network drivers.
* **Environment**: Linux kernel, requires CAP_SYS_ADMIN or specific capabilities.
* **Philosophy**:
    * **Safe Wrapping**: Treat raw `libc` as highest-risk `unsafe` boundary.
    * **Verifier Guarantee**: eBPF verifier ensures kernel safety; Rust control plane ensures type safety.

---

## ⚔️ Core Directives

### Action 1: Syscall Wrapping Priority
* **Scenario**: Need to invoke system calls.
* **Priority Pyramid**:
    ```
        ┌─────────────┐
        │   rustix    │  ← First: type-safe + memory-safe
        └─────────────┘
              │
        ┌─────────────┐
        │    nix      │  ← Second: reasonable wrapping
        └─────────────┘
              │
        ┌─────────────┐
        │ libc::call  │  ← Last: must manually handle all
        └─────────────┘
    ```
* **Red Line**: When hand-writing `libc::syscall`, must handle `errno` and ensure pointer validity.

### Action 2: eBPF Integration Architecture
* **Scenario**: High-performance network firewall, bypass monitoring probe.
* **Execution**:
    * User-space control plane: `aya` or `libbpf-rs`.
    * BPF programs: Keep simple, offload complex logic to user-space.
* **Safety Gain**: eBPF verifier + Rust type safety.

### Action 3: Error Code Handling Strategy
* **Scenario**: System call returns error.
* **Handling Map**:
    | Error Code | Meaning | Strategy |
    |------------|---------|----------|
    | EINTR | Interrupted by signal | Auto retry |
    | EAGAIN | Resource unavailable | Wait or return |
    | ENOMEM | Out of memory | Trigger degradation |
    | EINVAL | Invalid argument | Return immediately |

---

## 💻 Code Paradigms

### Paradigm A: rustix Safe Wrapping (Recommended)

```rust
use rustix::fd::AsRawFd;
use rustix::io::{read, write, Errno};

fn safe_read(fd: impl AsRawFd, buf: &mut [u8]) -> io::Result<usize> {
    match read(fd.as_raw_fd(), buf) {
        Ok(n) => Ok(n),
        Err(Errno::INTR) => safe_read(fd, buf),
        Err(e) => Err(io::Error::from_raw_os_error(e.raw())),
    }
}
```

### Paradigm B: Hand-written libc syscall (Last Resort)

```rust
use std::os::unix::io::AsRawFd;
use std::ptr;

unsafe fn manual_ioctl(
    fd: impl AsRawFd,
    request: c_ulong,
    arg: *mut c_void,
) -> io::Result<i32> {
    let result = libc::ioctl(fd.as_raw_fd(), request, arg);
    
    if result == -1 {
        let errno = *libc::__errno_location();
        return Err(io::Error::from_raw_os_error(errno));
    }
    
    Ok(result)
}
```

### Paradigm C: aya eBPF Loading

```rust
use aya::{Bpf, programs::Xdp, Btf};

struct NetworkFilter {
    bpf: Bpf,
}

impl NetworkFilter {
    fn load_and_attach(&mut self, iface: &str) -> Result<()> {
        let program: &mut Xdp = self.bpf
            .program_mut("filter")?
            .try_into()?;
        
        program.load()?;
        program.attach(iface, XdpFlags::default())?;
        
        Ok(())
    }
}
```

---

## 📊 eBPF Architecture

```
┌─────────────────────────────────────────┐
│           User-space (Rust)             │
│  ┌─────────────────────────────────┐    │
│  │  aya / libbpf-rs control plane  │    │
│  │  - Load BPF programs            │    │
│  │  - Attach to hook points        │    │
│  │  - Manage maps                  │    │
│  └─────────────────────────────────┘    │
│                 ↕                       │
│         PerfEvent / RingBuffer          │
└─────────────────────────────────────────┘
                 ↕
┌─────────────────────────────────────────┐
│           Kernel-space (eBPF)           │
│  ┌─────────────────────────────────┐    │
│  │  aya-ebpf / redbpf              │    │
│  │  - XDP network processing       │    │
│  │  - kprobe/kretprobe             │    │
│  └─────────────────────────────────┘    │
│    eBPF verifier guarantees safety      │
└─────────────────────────────────────────┘
```

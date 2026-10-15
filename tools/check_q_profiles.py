#!/usr/bin/env python3
"""Exercise fixed target layouts, finite bridges, and actual QEMU machine entry."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess

CORE = Path(__file__).resolve().parents[1]
FIXTURES = CORE / 'tests/compiler'


def require(value, message):
    if not value:
        raise ValueError(message)


def run(command, cwd, timeout=180):
    result = subprocess.run(list(map(str, command)), cwd=cwd, capture_output=True,
                            text=True, timeout=timeout)
    return {'command': list(map(str, command)), 'exit': result.returncode,
            'stdout': result.stdout, 'stderr': result.stderr}


def layout(data, profile):
    require(data[:4] == b'\x7fELF', 'not ELF')
    entry, phoff = struct.unpack_from('<QQ', data, 24)
    size, count = struct.unpack_from('<HH', data, 54)
    segments = [struct.unpack_from('<IIQQQQQQ', data, phoff + i * size)
                for i in range(count)]
    require(len(segments) == 3 and all(s[0] == 1 for s in segments), 'three PT_LOAD')
    base = {'linux-user': 0x10000, 'suba-user': 0x10000,
            'suba-boot': 0x80200000, 'suba-kernel': 0x80400000}[profile]
    window = 2 * 1024**2 if profile == 'suba-boot' else 64 * 1024**2
    require(min(s[3] for s in segments) == entry == base, 'lowest segment/entry mismatch')
    require([s[1] for s in segments] == [5, 4, 6], 'RX/R/RW permissions')
    for segment in segments:
        _, _, offset, virtual, physical, filesz, memsz, align = segment
        require(virtual == physical and virtual >= base, 'virtual/physical/base')
        require(memsz >= filesz and virtual + memsz <= base + window, 'window')
        require(offset + filesz <= len(data) and offset % align == virtual % align, 'file range')
    if profile in ('suba-boot', 'suba-kernel'):
        require(segments[2][6] >= 131072, 'stack missing from RW memory')
        require(struct.unpack_from('<I', data, segments[0][2])[0] == 0x10017073,
                'machine entry must disable SIE before using old stack')
    return {'entry': entry, 'segments': segments, 'sha256': hashlib.sha256(data).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, required=True)
    parser.add_argument('--record-dir', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    record = args.record_dir.resolve()
    record.mkdir(parents=True, exist_ok=False)
    qemu = shutil.which('qemu-riscv64')
    machine = shutil.which('qemu-system-riscv64')
    require(qemu and machine, 'both QEMU runners required')
    report = {'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
              'cases': [], 'passed': False}
    source = (FIXTURES / 'k0-profile-machine.vt').read_text()
    assembly = (FIXTURES / 'k0-profile-machine.s0').read_text()
    user = (FIXTURES / 'k0-profile-user.vt').read_text()

    def build(name, profile, text=None, asm=None, bindings=None, entry=None, expected=None):
        work = record / name
        work.mkdir()
        privileged = profile in ('suba-boot', 'suba-kernel')
        if text is None:
            text = source if privileged else user
        if entry is None:
            entry = ('profile_probe::boot' if profile == 'suba-boot' else
                     'profile_probe::kernel' if privileged else 'profile_user::run')
        if bindings is None:
            bindings = 'bind panic=profile_probe::panic\n' if privileged else ''
            if profile == 'suba-kernel':
                bindings += 'bind trap_dispatch=profile_probe::trap\n'
        if asm is None and privileged:
            asm = assembly
            if profile == 'suba-boot':
                asm = asm.replace('jump __suba_trap_dispatch', 'jalr x0, 0(x1)')
        if asm is None and profile == 'suba-user':
            # Encoding/relocation check only; real suba exit is verified by the
            # kernel integration. This test stub deliberately cannot succeed.
            asm = '__suba_exit:\njump __suba_exit\n'
        (work / 'probe.vt').write_text(text)
        manifest = (f'format 1\nprofile {profile}\nentry {entry}\n{bindings}'
                    'source probe.vt\n')
        if asm is not None:
            (work / 'probe.s0').write_text(asm)
            manifest += 'assembly probe.s0\n'
        (work / 'probe.build').write_text(manifest + 'output probe.elf\n')
        result = run([qemu, '-s', '1048576', compiler, 'build', 'probe.build'], work)
        item = {'name': name, **result}
        report['cases'].append(item)
        if expected:
            require(result['exit'] != 0 and expected in result['stderr'],
                    f'{name}: expected {expected}, got {result}')
            require(not (work / 'probe.elf').exists(), f'{name}: rejected image published')
        else:
            require(result['exit'] == 0, f'{name}: build failed {result}')
            item['layout'] = layout((work / 'probe.elf').read_bytes(), profile)
        return work, item

    try:
        for profile in ('linux-user', 'suba-user', 'suba-boot', 'suba-kernel'):
            work, item = build(profile, profile)
            if profile == 'linux-user':
                item['execution'] = run([qemu, work / 'probe.elf'], work)
                require(item['execution']['exit'] == 37, 'linux runtime/relocations')
                if args.baseline:
                    expected_image = (work / 'probe.elf').read_bytes()
                    (work / 'probe.elf').unlink()
                    baseline = run([qemu, '-s', '1048576', args.baseline.resolve(),
                                    'build', 'probe.build'], work)
                    item['baseline'] = baseline
                    require(baseline['exit'] == 0, 'baseline build failed')
                    require((work / 'probe.elf').read_bytes() == expected_image,
                            'Linux ELF changed relative to accepted baseline')
            if profile in ('suba-boot', 'suba-kernel'):
                item['execution'] = run([machine, '-machine', 'virt', '-m', '512M',
                    '-smp', '1', '-nographic', '-bios', 'default', '-kernel', work / 'probe.elf',
                    '-no-reboot'], work, timeout=20)
                require(item['execution']['exit'] == 0, f'{profile}: machine failed')
                rw = item['layout']['segments'][2]
                poison = work / 'poison.bin'
                poison.write_bytes(b'\xa5' * (rw[6] - rw[5]))
                command = item['execution']['command'] + [
                    '-device', f'loader,file={poison},addr={rw[3] + rw[5]}']
                poisoned = run(command, work, timeout=20)
                report['cases'].append({'name': profile+'-poisoned-bss', **poisoned})
                require(poisoned['exit'] == 0, f'{profile}: BSS was not cleared')
        checked = source.replace('reason != 102_u32', 'reason != 101_u32') + """
pub fn fail(hart: usize, dtb: usize) -> u32 {
    let values: [1]u8 = [1];
    unsafe { return values[2_usize] as u32; }
}
"""
        work, item = build('checked-panic', 'suba-boot', text=checked, entry='profile_probe::fail')
        item['execution'] = run([machine, '-machine', 'virt', '-m', '512M', '-smp', '1',
            '-nographic', '-bios', 'default', '-kernel', work / 'probe.elf', '-no-reboot'], work, 20)
        require(item['execution']['exit'] == 0, 'checked panic did not reach bound hook')
        build('unknown-profile', 'other', expected='I_UNSUPPORTED')
        build('user-binding', 'linux-user', bindings='bind panic=x\n', expected='I_UNSUPPORTED')
        build('suba-user-binding', 'suba-user', bindings='bind panic=x\n', expected='I_UNSUPPORTED')
        build('missing-user-exit', 'suba-user', asm='', expected='E_LINK')
        build('unknown-binding', 'suba-boot', bindings='bind export=x\n', expected='I_UNSUPPORTED')
        build('duplicate-binding', 'suba-boot', bindings='bind panic=x\nbind panic=x\n', expected='E_PARSE')
        build('missing-panic', 'suba-boot', bindings='', expected='E_NAME')
        build('missing-trap', 'suba-kernel', bindings='bind panic=profile_probe::panic\n', expected='E_NAME')
        build('boot-trap', 'suba-boot', bindings='bind panic=profile_probe::panic\nbind trap_dispatch=profile_probe::trap\n', expected='I_UNSUPPORTED')
        build('wrong-entry', 'suba-boot', entry='profile_probe::panic', expected='E_TYPE')
        build('wrong-panic', 'suba-boot', bindings='bind panic=profile_probe::trap\n', expected='E_TYPE')
        build('unsafe-panic', 'suba-boot', text=source.replace('pub fn panic', 'pub unsafe fn panic'), expected='E_TYPE')
        build('wrong-trap', 'suba-kernel', bindings='bind panic=profile_probe::panic\nbind trap_dispatch=profile_probe::panic\n', expected='E_TYPE')
        build('missing-halt', 'suba-boot', asm=assembly.replace('__suba_halt:', '__wrong_halt:').replace('jump __suba_trap_dispatch', 'jalr x0, 0(x1)'), expected='E_LINK')
        build('duplicate-trap-symbol', 'suba-kernel', asm=assembly+'\n__suba_trap_dispatch:\njalr x0, 0(x1)\n', expected='E_PARSE')
        build('boot-window', 'suba-boot', text=source+'\nstatic mut HUGE: [2097152]u8 = zero;\n', expected='I_LIMIT')
        build('kernel-window', 'suba-kernel', text=source+'\nstatic mut HUGE: [67108864]u8 = zero;\n', expected='I_LIMIT')
        # The independent ELF reader must reject the exact historical boot
        # counterexamples even when the header claims the expected e_entry.
        original = (record / 'suba-boot/probe.elf').read_bytes()
        for name, offset, value in [('low-load', 64 + 16, 0x10000),
                                    ('entry-not-first-byte', 24, 0x80200004)]:
            corrupt = bytearray(original)
            struct.pack_into('<Q', corrupt, offset, value)
            try:
                layout(corrupt, 'suba-boot')
            except ValueError:
                report['cases'].append({'name': name, 'rejected': True})
            else:
                raise ValueError(f'{name}: checker accepted bad boot image')
        report['passed'] = True
    finally:
        report['tool_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        report['fixture_sha256'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
            for p in FIXTURES.glob('k0-profile-*')}
        (record / 'verification.json').write_text(json.dumps(report, indent=2)+'\n')
    print(f"profiles: {len(report['cases'])} cases passed")


if __name__ == '__main__':
    main()

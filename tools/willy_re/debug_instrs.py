"""Debug: dump raw instructions for handler 0 of slot 30."""
import struct
from willy_re.director.parser import DirectorFile
from willy_re.lingo.bytecode import parse_lscr, OpCode

with DirectorFile("../../game/Movies/02.DXR") as df:
    df.parse()
    names = df.name_table

    # Print some specific name indices
    for i in [0, 3, 8, 9, 12, 112, 170, 171, 172]:
        if i < len(names):
            print(f"  names[{i}] = {names[i]!r}")

    print()

    for idx, entry in enumerate(df.entries):
        if entry.type != "Lscr" or idx != 30:
            continue
        data = df.get_entry_data(idx)
        script = parse_lscr(data, names)

        print(f"Properties: {script.property_names}")
        print(f"Globals: {script.global_names}")

        for hi, h in enumerate(script.handlers[:3]):
            print(f"\nHandler[{hi}]: {h.name}({', '.join(h.arg_names)})")
            print(f"  locals: {h.local_names}, arg_count={h.arg_count}, local_count={h.local_count}")
            bc = data[h.bytecode_offset : h.bytecode_offset + h.bytecode_length]
            print(f"  Raw: {' '.join(f'{b:02X}' for b in bc)}")
            for ins in h.instructions:
                base = ins.opcode - 0x40 if ins.opcode >= 0x80 else ins.opcode
                extra = ""
                if base in (0x48, 0x49, 0x4A, 0x4B, 0x4D, 0x4E, 0x4F, 0x50):
                    nm = names[ins.arg] if 0 <= ins.arg < len(names) else "?"
                    extra = f"  name[{ins.arg}]={nm!r}"
                print(f"  {ins.offset:04X}: {ins.name} arg={ins.arg}{extra}")

        # Also hex dump first handler record
        func_off = struct.unpack_from(">I", data, 0x4A)[0]
        rec = data[func_off:func_off + 42]
        print(f"\nHandler record 0 raw (at offset {func_off}):")
        print(" ".join(f"{b:02X}" for b in rec))
        break

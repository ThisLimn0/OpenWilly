"""Check if bytecode args for variable-access opcodes need /6 stride."""
import struct
from willy_re.director.parser import DirectorFile
from willy_re.lingo.bytecode import parse_lscr

with DirectorFile("../../game/Movies/02.DXR") as df:
    df.parse()
    names = df.name_table

    div6_count = 0
    not_div6_count = 0
    total = 0

    for idx, entry in enumerate(df.entries):
        if entry.type != "Lscr":
            continue
        data = df.get_entry_data(idx)
        script = parse_lscr(data, names)
        for h in script.handlers:
            for ins in h.instructions:
                base = ins.opcode - 0x40 if ins.opcode >= 0x80 else ins.opcode
                if base in (0x48, 0x49, 0x4A, 0x4B, 0x4D, 0x4E, 0x4F, 0x50):
                    total += 1
                    if ins.arg == 0:
                        div6_count += 1
                    elif ins.arg % 6 == 0:
                        div6_count += 1
                    else:
                        not_div6_count += 1

    print(f"Total variable-access opcodes: {total}")
    print(f"Args divisible by 6: {div6_count}")
    print(f"Args NOT divisible by 6: {not_div6_count}")
    if not_div6_count > 0:
        print("CONCLUSION: /6 stride is IMPOSSIBLE - args are DIRECT Lnam name indices")
    else:
        print("CONCLUSION: All args divisible by 6 - stride might apply")

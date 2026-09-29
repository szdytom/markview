"""Read JSON headers and the raw PNG body attached to a tile response."""
import json


def read_response(stream):
    line = stream.readline()
    if not line:
        return None
    answer = json.loads(line)
    size = len(line)
    if "tile" in answer:
        tile = answer["tile"]
        assert tile["encoding"] == "png" and "png" not in tile
        remaining = tile["bytes"]
        assert isinstance(remaining, int) and 0 < remaining <= 64 * 1024 * 1024
        chunks = []
        while remaining:
            chunk = stream.read(remaining)
            if not chunk:
                raise EOFError("Truncated PNG response")
            chunks.append(chunk)
            remaining -= len(chunk)
        tile["png"] = b"".join(chunks)
        size += tile["bytes"]
    answer["_wire_bytes"] = size
    return answer

"""Widens the card reader's float16 weights to float32, so tract can run the model.

Stripe's CardScan digit detector (darknite_1_1_1_16.tflite) keeps its weights as float16
constants, each behind a DEQUANTIZE op that tract-tflite does not have. Every such op is
removed and its output tensor given the widened weights directly. Float16 to float32 is
exact, so the graph computes what it did before.

    pip install ai-edge-litert numpy
    python3 tools/data/widen_cardscan.py darknite_1_1_1_16.tflite data/cardscan/darknite.tflite

ai-edge-litert is only for the schema bindings it ships; nothing here runs the model.
"""

import sys

import flatbuffers
import numpy as np
from ai_edge_litert import schema_py_generated as schema

DEQUANTIZE = schema.BuiltinOperator.DEQUANTIZE


def widen(source: bytes) -> bytes:
    model = schema.ModelT.InitFromObj(schema.Model.GetRootAs(source, 0))
    codes = [max(code.builtinCode, code.deprecatedBuiltinCode) for code in model.operatorCodes]
    widened = 0
    for graph in model.subgraphs:
        kept = []
        for op in graph.operators:
            if codes[op.opcodeIndex] != DEQUANTIZE:
                kept.append(op)
                continue
            src = graph.tensors[op.inputs[0]]
            dst = graph.tensors[op.outputs[0]]
            if src.type != schema.TensorType.FLOAT16 or dst.type != schema.TensorType.FLOAT32:
                raise SystemExit(f"DEQUANTIZE from {src.type} to {dst.type} is not a float16 constant")
            data = model.buffers[src.buffer].data
            if data is None or len(data) == 0:
                raise SystemExit("a DEQUANTIZE input has no constant data")
            values = np.frombuffer(bytes(bytearray(data)), dtype="<f2").astype("<f4")
            buffer = schema.BufferT()
            buffer.data = np.frombuffer(values.tobytes(), dtype=np.uint8)
            model.buffers.append(buffer)
            dst.buffer = len(model.buffers) - 1
            # The float16 copy is no longer read by anything.
            model.buffers[src.buffer].data = None
            widened += 1
        graph.operators = kept
    builder = flatbuffers.Builder(len(source) * 3)
    builder.Finish(model.Pack(builder), file_identifier=b"TFL3")
    print(f"widened {widened} constants", file=sys.stderr)
    return bytes(builder.Output())


if __name__ == "__main__":
    with open(sys.argv[1], "rb") as f:
        out = widen(f.read())
    with open(sys.argv[2], "wb") as f:
        f.write(out)

#!/usr/bin/env python3
import cadquery as cq
from builder import CaseBuilder

def main():
    builder = CaseBuilder()
    print("Creating stacked case model...")
    assembly = builder.build()
    if assembly is None:
        print("Case creation failed. Cannot export.")
        return None

    # Print dimensions in inches
    ext_w = builder.external_width / 25.4
    ext_d = builder.external_depth / 25.4
    ext_h = builder.external_height / 25.4
    int_w = builder.internal_width / 25.4
    int_d = builder.internal_depth / 25.4
    int_h = builder.total_internal_height / 25.4

    print(f"External Case Dimensions (WxDxH): {ext_w:.2f} x {ext_d:.2f} x {ext_h:.2f} inches")
    print(f"Internal Case Dimensions (WxDxH): {int_w:.2f} x {int_d:.2f} x {int_h:.2f} inches")
    print("Exporting model to eeg_case.step / .stl")

    case_body = assembly.objects["case_body"].obj
    cq.exporters.export(case_body, "eeg_case.step")
    cq.exporters.export(case_body, "eeg_case.stl")

    return assembly

if __name__ == "__main__":
    main()
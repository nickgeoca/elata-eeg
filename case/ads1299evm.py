import cadquery as cq
import math

# ADS1299EVM board parameters
BOARD_WIDTH = 81.28
BOARD_DEPTH = 88.9
BOARD_THICKNESS = 1.5748
PIN_HEIGHT_ABOVE_PCB = 6.10
PIN_DIAMETER = 1.0
PIN_CHANNEL_COUNT = 8

# Mount holes
MOUNT_HOLE_OFFSET_X = 5.08
MOUNT_HOLE_OFFSET_Y_BOTTOM = 5.08
MOUNT_HOLE_OFFSET_Y_TOP = 5.715
MOUNT_HOLE_DIAMETER = 3.81  # 150 mils

# Pin positions (relative to bottom-left)
PIN_OFFSET_X = 5.08
PIN_BASE_OFFSET_Y = 29.21
PIN_SPACING_Y = 5.08
BIAS_PIN_OFFSET_Y = 29.21
REF_PIN_OFFSET_Y = 24.13

def mount_hole_positions():
    # Returns list of (x, y) tuples for mount holes relative to board bottom-left corner
    mount_hole_spacing_x = BOARD_WIDTH - 2 * MOUNT_HOLE_OFFSET_X
    mount_hole_spacing_y = BOARD_DEPTH - MOUNT_HOLE_OFFSET_Y_BOTTOM - MOUNT_HOLE_OFFSET_Y_TOP
    return [
        (MOUNT_HOLE_OFFSET_X, MOUNT_HOLE_OFFSET_Y_BOTTOM),  # Bottom-Left (x- side)
        (MOUNT_HOLE_OFFSET_X, MOUNT_HOLE_OFFSET_Y_BOTTOM + mount_hole_spacing_y),  # Top-Left (x- side)
    ]

def positive_channel_pin_positions():
    # Returns list of (x, y) tuples for Ch(n)+ pins relative to board bottom-left corner
    positions = []
    for n in range(1, PIN_CHANNEL_COUNT + 1):
        y_pos = PIN_BASE_OFFSET_Y + n * PIN_SPACING_Y
        positions.append((PIN_OFFSET_X, y_pos))
    return positions

def create_ads1299_step():
    """
    Generates and exports a STEP file for the ADS1299EVM board only (no case, no LCD).
    Returns a CadQuery Assembly for visualization.
    """
    # Create PCB centered at (0,0,0)
    pcb_shape = cq.Workplane("XY").box(
        BOARD_WIDTH, BOARD_DEPTH, BOARD_THICKNESS, centered=(True, True, True)
    ).val()
    if not pcb_shape:
        print("Failed to create ADS1299EVM PCB for STEP export.")
        return None

    # Create mount holes
    mount_positions = mount_hole_positions()
    centered_mount_positions = [
        (x - BOARD_WIDTH / 2, y - BOARD_DEPTH / 2) for (x, y) in mount_positions
    ]
    mount_holes = (
        cq.Workplane("XY")
        .pushPoints(centered_mount_positions)
        .circle(MOUNT_HOLE_DIAMETER / 2)
        .extrude(BOARD_THICKNESS * 2, both=True)  # Extrude symmetrically to guarantee through-hole
    )
    pcb_with_holes = cq.Workplane(pcb_shape).cut(mount_holes).val()

    # Create pins (positive channels + bias + ref)
    ch_pin_positions = positive_channel_pin_positions()
    bias_pin_pos = (PIN_OFFSET_X, BIAS_PIN_OFFSET_Y)
    ref_pin_pos = (PIN_OFFSET_X, REF_PIN_OFFSET_Y)
    all_pin_positions = ch_pin_positions + [bias_pin_pos, ref_pin_pos]
    centered_pin_positions = [
        (x - BOARD_WIDTH / 2, y - BOARD_DEPTH / 2) for (x, y) in all_pin_positions
    ]
    pin_start_z = BOARD_THICKNESS / 2
    pins = (
        cq.Workplane("XY", origin=(0, 0, pin_start_z))
        .pushPoints(centered_pin_positions)
        .circle(PIN_DIAMETER / 2)
        .extrude(PIN_HEIGHT_ABOVE_PCB)
    ).val()

    # Fuse pins to PCB
    if pins:
        combined_shape = pcb_with_holes.fuse(pins)
    else:
        combined_shape = pcb_with_holes

    # Rotate 90 deg around Z as requested
    rotated_shape_z90 = combined_shape.rotate((0, 0, 0), (0, 0, 1), 90)

    # Create a simple assembly for visualization
    assembly = cq.Assembly()
    assembly.add(rotated_shape_z90, name="ads1299_board", color=cq.Color("blue")) # Use rotated_shape_z90
    # Export as STEP
    cq.exporters.export(rotated_shape_z90, "ads1299evm.step") # Use rotated_shape_z90
    print("Exported ads1299evm.step")
    return assembly

if __name__ == "__main__":
    create_ads1299_step()
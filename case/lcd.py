import cadquery as cq

# Model origin and orientation:
# (0, 0, 0) is the center of the top face of the LCD board.
# X and Y axes are centered; board extends equally in both directions from center.
# Z = 0 is the top face, Z = -7.7 mm is the bottom face.

# LCD board parameters
# PCB dimensions (board under LCD panel)
PCB_WIDTH = 121.11
PCB_HEIGHT = 77.93
PCB_THICKNESS = 1.6  # PCB thickness only

# LCD panel dimensions (top layer)
PANEL_WIDTH = 120.5
PANEL_HEIGHT = 75.65
PANEL_THICKNESS = 6.1

MOUNT_HOLE_DIAMETER = 2.5
STUD_DIAMETER = 5.0
STUD_HEIGHT = 4.0
DEFAULT_OUTER_HOLE_INSET = 5.0

def inner_mount_hole_positions():
    # Returns list of (x, y) tuples for inner holes relative to board bottom-left corner
    inner_x_offset = 20
    inner_y_offset_bottom = 21.50
    inner_y_offset_top = 77.93 - 7.43  # 70.50
    inner_x_spacing = 58.00
    return [
        (inner_x_offset, inner_y_offset_top),  # Top-Left
        (inner_x_offset + inner_x_spacing, inner_y_offset_top),  # Top-Right
        (inner_x_offset, inner_y_offset_bottom),  # Bottom-Left
        (inner_x_offset + inner_x_spacing, inner_y_offset_bottom),  # Bottom-Right
    ]

def outer_mount_hole_positions(inset=DEFAULT_OUTER_HOLE_INSET):
    # Returns list of (x, y) tuples for outer holes relative to board bottom-left corner
    return [
        (inset, inset),  # Bottom-Left
        (PCB_WIDTH - inset, inset),  # Bottom-Right
        (inset, PCB_HEIGHT - inset),  # Top-Left
        (PCB_WIDTH - inset, PCB_HEIGHT - inset),  # Top-Right
    ]

def create_lcd_step():
    """
    Generates and exports a STEP file for the LCD assembly (Panel + PCB).
    Adds mount holes and studs to the PCB.
    Adds a screen feature to the Panel.
    Returns a CadQuery Assembly for visualization with separate colors.
    """
    # Screen parameters
    SCREEN_WIDTH = 109.2 + 0.2  # 109.4 mm
    SCREEN_HEIGHT = 66.05 + 0.2  # 66.25 mm
    SCREEN_X_OFFSET = 5.65  # from left and right (unused currently, but kept for reference)
    SCREEN_Y_OFFSET_TOP = 2.7  # from top

    # Calculate required Y offset to align panel top with PCB top
    panel_y_offset = (PCB_HEIGHT / 2) - (PANEL_HEIGHT / 2)

    # --- Create Base Parts ---
    # LCD panel (top) - Origin at top center (Z=0), shifted up to align top edges
    lcd_panel = cq.Workplane("XY").box(
        PANEL_WIDTH, PANEL_HEIGHT, PANEL_THICKNESS, centered=(True, True, False)
    ).translate((0, panel_y_offset, -PANEL_THICKNESS)).val() # Apply Y offset

    # PCB (bottom) - Origin at its top center (Z=-PANEL_THICKNESS)
    pcb = cq.Workplane("XY").box(
        PCB_WIDTH, PCB_HEIGHT, PCB_THICKNESS, centered=(True, True, False)
    ).translate((0, 0, -(PANEL_THICKNESS + PCB_THICKNESS))).val()

    # --- Modify PCB ---
    # Mount hole positions (inner + outer)
    inner_positions = inner_mount_hole_positions()
    outer_positions = outer_mount_hole_positions()
    all_positions = inner_positions + outer_positions

    # Center the positions relative to the PCB's center
    centered_positions = [
        (x - PCB_WIDTH / 2, y - PCB_HEIGHT / 2) for (x, y) in all_positions
    ]

    # Create through-holes in PCB
    hole_cylinders = (
        cq.Workplane("XY")
        .pushPoints(centered_positions)
        .circle(MOUNT_HOLE_DIAMETER / 2)
        .extrude(-PCB_THICKNESS) # Only extrude through PCB thickness
        .translate((0, 0, -PANEL_THICKNESS)) # Start holes at top of PCB layer
    )
    pcb_with_holes = cq.Workplane(pcb).cut(hole_cylinders).val()

    # Create studs at all mount hole positions, protruding from the bottom face of PCB
    studs = (
        cq.Workplane("XY")
        .pushPoints(centered_positions)
        .circle(STUD_DIAMETER / 2)
        .extrude(-STUD_HEIGHT)
        .translate((0, 0, -(PANEL_THICKNESS + PCB_THICKNESS))) # Position studs at PCB bottom
        .val()
    )
    pcb_with_holes_and_studs = cq.Workplane(pcb_with_holes).union(studs).val()

    # Drill holes through the studs
    stud_hole_cylinders = (
        cq.Workplane("XY")
        .pushPoints(centered_positions)
        .circle(MOUNT_HOLE_DIAMETER / 2)
        .extrude(-STUD_HEIGHT) # Extrude downwards through stud height
        .translate((0, 0, -(PANEL_THICKNESS + PCB_THICKNESS))) # Start holes at bottom face of PCB
    )
    final_pcb = cq.Workplane(pcb_with_holes_and_studs).cut(stud_hole_cylinders).val()

    # --- Modify Panel ---
    # Calculate screen center position
    screen_center_x = 0  # centered horizontally

    # Calculate screen center Y relative to PCB center (as before)
    pcb_top_y = PCB_HEIGHT / 2
    screen_top_edge_y_rel_pcb_center = pcb_top_y - SCREEN_Y_OFFSET_TOP # Y coord relative to PCB center
    screen_bottom_edge_y_rel_pcb_center = screen_top_edge_y_rel_pcb_center - SCREEN_HEIGHT
    screen_center_y_rel_pcb_center = (screen_top_edge_y_rel_pcb_center + screen_bottom_edge_y_rel_pcb_center) / 2

    # Calculate screen center Y relative to the *panel's* center
    # The panel's center is now at (0, panel_y_offset, -PANEL_THICKNESS/2)
    # The workplane for cutting is centered on the panel's top face: (0, panel_y_offset, 0)
    # We need the screen center relative to this workplane origin.
    screen_center_y_rel_panel_center = screen_center_y_rel_pcb_center - panel_y_offset

    # Add a shallow pocket (0.2mm deep) to represent the screen on the panel's top face
    final_panel = (
        cq.Workplane(lcd_panel)
        .faces(">Z") # Select the top face of the panel
        .workplane(centerOption="CenterOfMass") # Workplane centered on the top face's CoM
        .center(screen_center_x, screen_center_y_rel_panel_center) # Center relative to panel workplane
        .rect(SCREEN_WIDTH, SCREEN_HEIGHT)
        .cutBlind(-0.2) # Cut 0.2mm into the panel
    ).val()

    # --- Create Screen Overlay ---
    screen_overlay = (
        cq.Workplane("XY")
        .center(screen_center_x, screen_center_y_rel_panel_center) # Center relative to panel workplane
        .rect(SCREEN_WIDTH, SCREEN_HEIGHT)
        .workplane(offset=0) # Workplane at Z=0
        .extrude(0.01) # Make it very thin, just for visualization
        .translate((0, 0, 0))  # Position at z=0, top of LCD panel
    )

    # --- Create Assembly for Visualization ---
    assembly = cq.Assembly()
    assembly.add(final_panel, name="lcd_panel", color=cq.Color("darkgreen"))
    assembly.add(final_pcb, name="pcb", color=cq.Color("green"))
    assembly.add(screen_overlay, name="screen_overlay", color=cq.Color("blue"))

    # --- Export Combined Shape as STEP ---
    # Union the final parts for a single STEP file export
    combined_shape_for_export = cq.Workplane(final_panel).union(final_pcb).val()
    if not combined_shape_for_export:
         print("Failed to create combined shape for STEP export.")
         # Optionally add fallback or error handling
         combined_shape_for_export = final_panel # Export something at least

    cq.exporters.export(combined_shape_for_export, "lcd.step")
    print("Exported lcd.step with PCB (green) and Panel (darkgreen), mount holes, studs, and screen pocket")

    return assembly

if __name__ == "__main__":
    assembly_result = create_lcd_step()
    # If running interactively (e.g., in CQ-Editor or Jupyter), you might want to show the assembly:
    # show_object(assembly_result) # Requires show_object to be defined/imported
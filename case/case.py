import cadquery as cq
from cadquery import importers
import os

# --- Component Dimension Classes ---
# Coordinate System Reference (relative to component origin):
# X = width (horizontal, left-to-right)
# Y = depth (front-to-back)
# Z = height (bottom-to-top)

class Pi5:
    width = 85.0
    depth = 56.0 # Official dimension (Y-axis in this layout)
    height = 20.0 # Estimated height including components/ports for clearance
    mount_hole_spacing_x = 58.0
    mount_hole_spacing_y = 49.0
    mount_hole_offset_x = 3.5 # Offset from the 'left' edge (min X) based on user measurement
    mount_hole_count = 4
    # Calculate offset_y directly
    mount_hole_offset_y = (depth - mount_hole_spacing_y) / 2 # 3.5
    bounding_box = (width, depth, height)

class LCD:
    board_width = 121.11
    board_depth = 77.93
    board_thickness = 2.0
    total_depth_clearance = 12.0 # Total thickness needed including components on front (Z-axis)
    va_width = 109.20
    va_depth = 66.05 # Viewable area Y-dimension
    mount_hole_diameter = 2.5 # Diameter for M2.5 screw clearance

    # Inner hole offsets (relative to bottom-left corner of board)
    inner_x_offset = 15.03
    inner_y_offset_bottom = 16.50
    inner_y_offset_top = 77.93 - 2.43 # 75.50
    inner_x_spacing = 58.00
    # Calculate Y spacing: inner_y_offset_top - inner_y_offset_bottom = 59.00

    bounding_box = (board_width, board_depth, total_depth_clearance) # Use clearance as height for bounding box

    @classmethod
    def inner_mount_hole_positions_relative(cls):
        """Returns list of (x, y) tuples for inner holes relative to board bottom-left corner."""
        return [
            (cls.inner_x_offset, cls.inner_y_offset_top), # Top-Left
            (cls.inner_x_offset + cls.inner_x_spacing, cls.inner_y_offset_top), # Top-Right
            (cls.inner_x_offset, cls.inner_y_offset_bottom), # Bottom-Left
            (cls.inner_x_offset + cls.inner_x_spacing, cls.inner_y_offset_bottom), # Bottom-Right
        ]

    @classmethod
    def outer_mount_hole_positions_relative_lcd(cls, inset=5.0):
        """
        Returns list of (x, y) tuples for outer holes relative to LCD board
        bottom-left corner, inset from LCD edges.
        NOTE: These are likely NOT the final case fastening hole positions,
        which depend on the overall case dimensions.
        """
        return [
            (inset, inset), # Bottom-Left
            (cls.board_width - inset, inset), # Bottom-Right
            (inset, cls.board_depth - inset), # Top-Left
            (cls.board_width - inset, cls.board_depth - inset), # Top-Right
        ]

class ADS1299:
    # Dimensions based on initial layout assumptions (may need refinement)
    # Assuming similar footprint to LCD for side-by-side placement initially
    board_width = 121.11 # Placeholder - Adjust if known
    board_depth = 77.93 # Placeholder - Adjust if known
    height = None # Height not specified
    # Mounting info based on feedback (assuming 2 holes vertically aligned?)
    # mount_hole_spacing_x = 58.0 # X spacing not provided
    mount_hole_spacing_y = 67.93 # Vertical spacing
    # mount_hole_from_side = 15.00 # X offset not provided
    mount_hole_from_top = 2.43 # Y offset from top
    mount_hole_from_bottom = 16.50 # Y offset from bottom
    mount_hole_count = 2 # Based on feedback
    bounding_box = (board_width, board_depth, height)

# --- Instantiate Components ---
pi5 = Pi5()
lcd = LCD()
ads1299 = ADS1299() # Instance created, though not used yet

# --- Case Configuration ---
wall_thickness = 2.0
base_thickness = 2.0
# total_internal_height = 15.0 # OLD - Now calculated dynamically
standoff_height_base = 2.0 # Height of standoffs from the case base to the bottom of the first component (LCD)
standoff_height_between = 5.0 # Gap between back of LCD board and front of Pi 5 board
standoff_diameter = 5.0
screw_hole_diameter = 2.7 # For M2.5 screws
counterbore_diameter = 5.0 # For M2.5 screw head
counterbore_depth = 1.5
component_clearance = 2.0 # General clearance around components
top_clearance = 2.0 # Clearance above the top component (Pi 5)

# --- Calculated Case Dimensions (Stacked Layout) ---
# Internal dimensions accommodate the LARGER of the width/depth footprints
internal_width = max(pi5.width, lcd.board_width) + 2 * component_clearance
internal_depth = max(pi5.depth, lcd.board_depth) + 2 * component_clearance
# Internal height accommodates base standoffs, LCD, gap, Pi, and top clearance
total_internal_height = (
    standoff_height_base
    + lcd.board_thickness # Just the LCD board thickness itself sits on standoffs
    + standoff_height_between
    + pi5.height # Estimated height of Pi 5
    + top_clearance
)

external_width = internal_width + 2 * wall_thickness
external_depth = internal_depth + 2 * wall_thickness
external_height = base_thickness + total_internal_height

# --- Component Positions (relative to the case origin: center of the external bottom face [0,0,0]) ---
internal_cavity_min_x = -internal_width / 2
internal_cavity_min_y = -internal_depth / 2
internal_cavity_min_z = base_thickness

# Center the components within the internal cavity footprint
# LCD Position (Bottom component)
lcd_base_pos_x = internal_cavity_min_x + (internal_width - lcd.board_width) / 2
lcd_base_pos_y = internal_cavity_min_y + (internal_depth - lcd.board_depth) / 2
lcd_base_pos_z = internal_cavity_min_z + standoff_height_base # LCD sits on base standoffs

# Pi 5 Position (Top component)
# Assume Pi 5 is centered relative to the LCD footprint for now
pi5_base_pos_x = internal_cavity_min_x + (internal_width - pi5.width) / 2
pi5_base_pos_y = internal_cavity_min_y + (internal_depth - pi5.depth) / 2
# Pi sits above the LCD board, separated by the 'between' standoff height
pi5_base_pos_z = lcd_base_pos_z + lcd.board_thickness + standoff_height_between

def create_case():
    """Generates the 3D model of the Pi 5 stacked behind the Touchscreen case."""
    # Access dimensions via class instances (pi5, lcd) and global config

    # --- Build the Outer Box ---
    outer_box = cq.Workplane("XY").box(external_width, external_depth, external_height, centered=(True, True, False))

    # --- Create Screen Cutout Shape ---
    # Calculate cutout center position relative to case origin (0,0,0)
    lcd_center_x = lcd_base_pos_x + lcd.board_width / 2
    lcd_center_y = lcd_base_pos_y + lcd.board_depth / 2
    cutout_center_x = lcd_center_x
    cutout_center_y = lcd_center_y
    cutout_width = lcd.va_width + 1.0
    cutout_depth = lcd.va_depth + 1.0
    screen_cutout_shape = (
        cq.Workplane("XY", origin=(0, 0, external_height)) # Workplane on top face
        .center(cutout_center_x, cutout_center_y) # Move to the calculated center
        .rect(cutout_width, cutout_depth)
        .extrude(-external_height) # Extrude downwards through the entire case height
    ).val()

    if not screen_cutout_shape:
        print("Error: Failed to create screen cutout shape.")
        return None

    # --- Cut Screen Opening from Outer Box FIRST ---
    outer_box_cut_result = outer_box.cut(screen_cutout_shape) # Keep the workplane
    outer_box_with_cutout_shape = outer_box_cut_result.val() # Get the resulting shape
    if not outer_box_with_cutout_shape: # Check if the shape creation was successful
         print("Error: Failed to cut screen cutout from outer box.")
         # Decide how to handle: return None, or try to continue with original outer_box?
         # Let's try returning None for now.
         return None

    # --- Create Inner Box for Hollowing ---
    inner_width_to_remove = external_width - 2 * wall_thickness
    inner_depth_to_remove = external_depth - 2 * wall_thickness
    inner_height_to_remove = external_height - base_thickness
    inner_box = (
        cq.Workplane("XY")
        .box(inner_width_to_remove, inner_depth_to_remove, inner_height_to_remove, centered=(True, True, False))
        .translate((0, 0, base_thickness))
    )

    # --- Hollow Out the Box (which already has the screen cutout) ---
    # Perform the hollowing cut on the shape that already has the screen cutout
    case_shell_shape = cq.Workplane(outer_box_with_cutout_shape).cut(inner_box).val()

    if not case_shell_shape:
        print("Error: Failed to create case shell shape after hollowing.")
        return None # Indicate failure

    # --- Add LCD Representation (Shape only for now) ---
    # Create a simple box shape for the LCD board itself
    lcd_board_shape_base = cq.Workplane("XY").box(
        lcd.board_width, lcd.board_depth, lcd.board_thickness, centered=(True, True, False)
    ).val()

    if not lcd_board_shape_base:
        print("Error: Failed to create base LCD board shape.")
        lcd_board_shape = None # Ensure variable exists
    else:
        # The LCD visual is just the base shape (solid box)
        lcd_board_shape = lcd_board_shape_base

    # Position the LCD board visual representation
    lcd_vis_pos_x = lcd_base_pos_x + lcd.board_width / 2 # Center of the board
    lcd_vis_pos_y = lcd_base_pos_y + lcd.board_depth / 2 # Center of the board
    lcd_vis_pos_z = lcd_base_pos_z # Bottom of the board sits at lcd_base_pos_z
    # We'll add this shape to the assembly later, correctly positioned
    lcd_board_location = cq.Location(cq.Vector(lcd_vis_pos_x, lcd_vis_pos_y, lcd_vis_pos_z))

    # --- Calculate Absolute Mounting Hole Positions for Standoffs/Base Holes ---
    # Get relative positions (bottom-left origin)
    lcd_relative_holes = lcd.inner_mount_hole_positions_relative()
    # Convert to absolute positions in the case coordinate system
    absolute_mount_hole_positions = [
        (lcd_base_pos_x + x, lcd_base_pos_y + y) for x, y in lcd_relative_holes
    ]

    # --- Create Standoff Shape ---
    # Height for INNER standoffs (Reverting to original height for now)
    # Height for OUTER standoffs (up to Pi base - keep original calculation)
    pi_standoff_total_height = pi5_base_pos_z - base_thickness
    standoffs_shape = (
        cq.Workplane("XY", origin=(0, 0, base_thickness)) # Workplane on internal floor
        .pushPoints(absolute_mount_hole_positions) # Use absolute positions
        .circle(standoff_diameter / 2)
        .extrude(pi_standoff_total_height) # Revert to extruding upwards to Pi 5 level
    ).val()

    if not standoffs_shape: # Renaming inner standoffs for clarity
        print("Error: Failed to create inner standoffs shape.")
        return None

    # --- Create OUTER Standoff Shape ---
    # Calculate outer standoff positions relative to LCD edges, then convert to absolute
    outer_standoff_inset = 5.0
    # Get positions relative to LCD bottom-left corner
    relative_outer_positions_lcd = lcd.outer_mount_hole_positions_relative_lcd(inset=outer_standoff_inset)
    # Convert to absolute positions in the case coordinate system
    absolute_outer_standoff_positions = [
        (lcd_base_pos_x + x, lcd_base_pos_y + y) for x, y in relative_outer_positions_lcd
    ]
    # Use same height and diameter as inner standoffs
    outer_standoffs_shape = (
        cq.Workplane("XY", origin=(0, 0, base_thickness)) # Workplane on internal floor
        .pushPoints(absolute_outer_standoff_positions) # Use absolute outer positions based on LCD edges
        .circle(standoff_diameter / 2)
        .extrude(pi_standoff_total_height) # Extrude upwards to Pi 5 level (same height)
    ).val()

    if not outer_standoffs_shape:
        print("Error: Failed to create outer standoffs shape.")
        return None

    # --- Fuse Shell (already hollowed and with screen cut) and BOTH Standoff Sets ---
    # Fuse inner standoffs first
    fused_body_inner = case_shell_shape.fuse(standoffs_shape)
    if not fused_body_inner:
        print("Error: Failed to fuse shell and inner standoffs.")
        return None
    # Fuse outer standoffs to the result
    fused_body = fused_body_inner.fuse(outer_standoffs_shape)
    if not fused_body:
        print("Error: Failed to fuse outer standoffs.")
        return None

    # --- Screen Cutout is already done ---

    # --- NO HOLES DRILLED IN CASE BODY ---
    # The fused_body (hollowed shell with screen cut + standoffs) is the final shape
    final_case_shape = fused_body

    # --- NO HOLES DRILLED IN BASE ---
    if not final_case_shape:
         print("Error: Final case shape is invalid after drilling holes or before assembly.")
         return None

    # --- Create Final Assembly ---
    assembly = cq.Assembly()

    # Add the final processed case body
    assembly.add(final_case_shape, name="case_body", color=cq.Color("lightgray"))

    # Add the LCD visual representation shape at its calculated location
    if lcd_board_shape:
        assembly.add(lcd_board_shape, name="lcd_board", color=cq.Color("darkgreen"), loc=lcd_board_location)

    # --- Add Pi Model to Assembly ---
    pi_model_loaded = False
    pi5_model_shape = None
    pi5_model_location = None
    try:
        pi5_step_path = os.path.join(os.path.dirname(__file__), "RaspberryPi5.step")
        if os.path.exists(pi5_step_path):
            pi5_step_imported = importers.importStep(pi5_step_path)
            if pi5_step_imported and pi5_step_imported.val():
                # Rotate model to be flat and upright
                pi5_rotated_flat = pi5_step_imported.rotate((0, 0, 0), (1, 0, 0), -90)
                pi5_rotated = pi5_rotated_flat.rotate((0, 0, 0), (0, 1, 0), 180)
                pi5_model_shape = pi5_rotated.val() # Get the shape
                if pi5_model_shape:
                    pi5_bb = pi5_model_shape.BoundingBox()
                    # Calculate placement position
                    pi_place_x = pi5_base_pos_x - pi5_bb.xmin
                    pi_place_y = pi5_base_pos_y - pi5_bb.ymin
                    pi_place_z = pi5_base_pos_z - pi5_bb.zmin
                    pi5_model_location = cq.Location(cq.Vector(pi_place_x, pi_place_y, pi_place_z))
                    pi_model_loaded = True
                    print(f"Successfully loaded and prepared RaspberryPi5.step")
                else:
                    print(f"Warning: Rotated RaspberryPi5.step resulted in an empty shape.")
            else:
                 print(f"Warning: Imported RaspberryPi5.step from {pi5_step_path} but it resulted in an empty shape.")
        else:
            print(f"Warning: RaspberryPi5.step not found at {pi5_step_path}")
    except Exception as e:
        print(f"Could not load or process RaspberryPi5.step for assembly: {e}")

    # Add the Pi model shape if loaded successfully
    if pi_model_loaded and pi5_model_shape and pi5_model_location:
        assembly.add(pi5_model_shape, name="pi5_model", color=cq.Color("red"), loc=pi5_model_location)

    # Return the complete assembly
    return assembly # Return the assembly object

# Keep main this way. No need for more SLOC
def main():
    output_filename_base = "eeg_case" # New name for stacked layout

    print("Creating stacked case model...")
    result = create_case() # Call the function to get the assembly

    if result is None:
        print("Case creation failed. Cannot export.")
        return None

    print(f"External Case Dimensions (WxDxH): {external_width / 25.4:.2f} x {external_depth / 25.4:.2f} x {external_height / 25.4:.2f} inches")
    print(f"Internal Case Dimensions (WxDxH): {internal_width / 25.4:.2f} x { internal_depth / 25.4:.2f} x {total_internal_height / 25.4:.2f} inches")
    print(f"Exporting model to {output_filename_base}.step / .stl")

    # Extract the case body shape from the assembly for export
    case_body_shape_to_export = None
    if "case_body" in result.objects:
        case_body_shape_to_export = result.objects["case_body"].obj
    else:
        print("Error: 'case_body' not found in assembly objects for export.")
        return result # Return assembly even if export fails

    if case_body_shape_to_export:
        try:
            cq.exporters.export(case_body_shape_to_export, f"{output_filename_base}.step")
            cq.exporters.export(case_body_shape_to_export, f"{output_filename_base}.stl")
        except Exception as e:
            print(f"Error during export: {e}")
    else:
        print("Error: Could not extract case body shape for export.")


    return result # Return the model/assembly object for Jupyter display

if __name__ == "__main__":
    model = main()
    # If running in CQ-Editor or Jupyter, 'model' can be displayed if it's not None
    # if model:
    #    show_object(model) # Requires CQ-Editor environment or similar

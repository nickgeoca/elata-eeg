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
    underside_clearance = 2.0 # Estimated clearance needed below PCB bottom for components/pins

class LCD:
    board_width = 121.11
    board_depth = 77.93
    board_thickness = 2.0
    total_depth_clearance = 12.0 # Total thickness needed including components on front (Z-axis)
    va_width = 109.20
    va_depth = 66.05 # Viewable area Y-dimension
    mount_hole_diameter = 2.5 # Diameter for M2.5 screw clearance

    # Inner hole offsets (relative to bottom-left corner of board)
    inner_x_offset = 43.11 # Offset of the left-most inner holes from the left board edge.
                           # Derived by ensuring the right-most holes are 20mm from the right edge:
                           # board_width (121.11) - right_edge_distance (20.0) - inner_x_spacing (58.0) = 43.11
    inner_y_offset_bottom = 21.50 # Measured from bottom edge
    inner_y_offset_top = 77.93 - 7.43 # 70.50 (Measured 7.43 from top edge)
    inner_x_spacing = 58.00
    # Calculate Y spacing: inner_y_offset_top - inner_y_offset_bottom = 49.00

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
LCD_OUTER_HOLE_INSET = 5.0 # Inset distance from LCD edges for outer mounting/standoff points
SCREEN_CUTOUT_CLEARANCE = 1.0 # Extra space added around the LCD viewable area for the cutout

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
lcd_top_z = lcd_base_pos_z + lcd.board_thickness # Z coordinate of the top surface of the LCD

# Pi 5 Position (Top component)
# Calculate Pi 5 position to align its bottom-left mount hole
# with the bottom-left *inner* mount hole/standoff of the LCD.
# The inner hole relative position is (lcd.inner_x_offset, lcd.inner_y_offset_bottom)
pi5_base_pos_x = (lcd_base_pos_x + lcd.inner_x_offset) - pi5.mount_hole_offset_x - 23.0 # Shift left 2mm
pi5_base_pos_y = (lcd_base_pos_y + lcd.inner_y_offset_bottom) - pi5.mount_hole_offset_y  # Shift up 2mm
# Pi sits above the LCD board, separated by the 'between' standoff height
pi5_base_pos_z = lcd_top_z + standoff_height_between

# --- Helper Functions for Case Creation ---

def create_screen_cutout_shape(lcd_base_pos_x, lcd_base_pos_y, lcd, external_height):
    """Creates the shape for the screen cutout."""
    lcd_center_x = lcd_base_pos_x + lcd.board_width / 2
    lcd_center_y = lcd_base_pos_y + lcd.board_depth / 2
    cutout_center_x = lcd_center_x
    cutout_center_y = lcd_center_y
    cutout_width = lcd.va_width + SCREEN_CUTOUT_CLEARANCE
    cutout_depth = lcd.va_depth + SCREEN_CUTOUT_CLEARANCE
    shape = (
        cq.Workplane("XY", origin=(0, 0, external_height)) # Workplane on top face
        .center(cutout_center_x, cutout_center_y) # Move to the calculated center
        .rect(cutout_width, cutout_depth)
        .extrude(-external_height) # Extrude downwards through the entire case height
    ).val()
    if not shape:
        print("Error: Failed to create screen cutout shape.")
    return shape

def create_outer_box_with_cutout(external_width, external_depth, external_height, screen_cutout_shape):
    """Creates the initial solid outer box and cuts the screen opening."""
    outer_box = cq.Workplane("XY").box(external_width, external_depth, external_height, centered=(True, True, False))
    if not screen_cutout_shape:
        print("Error: Screen cutout shape is invalid, cannot cut from outer box.")
        return outer_box.val() # Return original box if cutout failed

    outer_box_cut_result = outer_box.cut(screen_cutout_shape)
    outer_box_with_cutout_shape = outer_box_cut_result.val()
    if not outer_box_with_cutout_shape:
        print("Error: Failed to cut screen cutout from outer box.")
        return outer_box.val() # Return original box if cut failed
    return outer_box_with_cutout_shape

def create_hollow_shell(box_with_cutout, external_width, external_depth, external_height, wall_thickness, base_thickness):
    """Hollows out the provided box shape."""
    inner_width_to_remove = external_width - 2 * wall_thickness
    inner_depth_to_remove = external_depth - 2 * wall_thickness
    inner_height_to_remove = external_height - base_thickness
    inner_box = (
        cq.Workplane("XY")
        .box(inner_width_to_remove, inner_depth_to_remove, inner_height_to_remove, centered=(True, True, False))
        .translate((0, 0, base_thickness))
    )
    shell_shape = cq.Workplane(box_with_cutout).cut(inner_box).val()
    if not shell_shape:
        print("Error: Failed to create case shell shape after hollowing.")
    return shell_shape

def create_standoffs(origin_z, positions, diameter, height):
    """Creates a set of standoffs at the given positions and height."""
    shape = (
        cq.Workplane("XY", origin=(0, 0, origin_z))
        .pushPoints(positions)
        .circle(diameter / 2)
        .extrude(height)
    ).val()
    if not shape:
        print(f"Error: Failed to create standoffs starting at Z={origin_z}.")
    return shape
def drill_screw_holes(origin_z, positions, screw_diameter, height):
    """Creates the shape of screw holes (cylinders) at specified locations."""
    # This function creates the *shape* to be cut, not perform the cut itself.
    hole_cylinders = (
        cq.Workplane("XY", origin=(0, 0, origin_z))
        .pushPoints(positions)
        .circle(screw_diameter / 2) # Create circles for the holes
        .extrude(height) # Extrude them to the specified height/depth
    )
    if not hole_cylinders or not hole_cylinders.val():
        print(f"Warning: Failed to create screw hole cylinder geometry at Z={origin_z}.")
        return None
    return hole_cylinders.val() # Return the combined shape of the cylinders


def create_lcd_visual(lcd, lcd_base_pos_x, lcd_base_pos_y, lcd_base_pos_z):
    """Creates the visual representation of the LCD board."""
    shape = cq.Workplane("XY").box(
        lcd.board_width, lcd.board_depth, lcd.board_thickness, centered=(True, True, False)
    ).val()
    if not shape:
        print("Error: Failed to create base LCD board shape.")
        return None, None

    vis_pos_x = lcd_base_pos_x + lcd.board_width / 2
    vis_pos_y = lcd_base_pos_y + lcd.board_depth / 2
    vis_pos_z = lcd_base_pos_z
    location = cq.Location(cq.Vector(vis_pos_x, vis_pos_y, vis_pos_z))
    return shape, location

def load_pi_model(pi5_base_pos_x, pi5_base_pos_y, pi5_base_pos_z):
    """Loads, rotates, and positions the Raspberry Pi STEP model."""
    try:
        pi5_step_path = os.path.join(os.path.dirname(__file__), "RaspberryPi5.step")
        if not os.path.exists(pi5_step_path):
            print(f"Warning: RaspberryPi5.step not found at {pi5_step_path}")
            return None, None

        pi5_step_imported = importers.importStep(pi5_step_path)
        if not (pi5_step_imported and pi5_step_imported.val()):
            print(f"Warning: Imported RaspberryPi5.step from {pi5_step_path} but it resulted in an empty shape.")
            return None, None

        # Rotate the imported model:
        # 1. Rotate -90 degrees around X-axis to lay it flat (assuming it imports standing up)
        pi5_rotated_flat = pi5_step_imported.rotate((0, 0, 0), (1, 0, 0), -90)
        # 2. Rotate 180 degrees around Y-axis to orient ports towards the 'back' (positive Y)
        pi5_rotated = pi5_rotated_flat.rotate((0, 0, 0), (0, 1, 0), 180)
        pi5_model_shape = pi5_rotated.val()
        if not pi5_model_shape:
            print(f"Warning: Rotated RaspberryPi5.step resulted in an empty shape.")
            return None, None

        # Calculate the placement position based on the model's bounding box
        # after rotation. We want the model's minimum corner (xmin, ymin, zmin)
        # to align with the calculated pi5_base_pos_x/y/z, which represents
        # the desired bottom-left-front corner of the Pi in the case assembly.
        pi5_bb = pi5_model_shape.BoundingBox()
        pi_place_x = pi5_base_pos_x - pi5_bb.xmin
        pi_place_y = pi5_base_pos_y - pi5_bb.ymin
        # Adjust Z placement to align PCB bottom, not lowest component
        pi_place_z = pi5_base_pos_z - (pi5_bb.zmin + pi5.underside_clearance)
        pi5_model_location = cq.Location(cq.Vector(pi_place_x, pi_place_y, pi_place_z))
        print(f"Successfully loaded and prepared RaspberryPi5.step")
        return pi5_model_shape, pi5_model_location

    except Exception as e:
        print(f"Could not load or process RaspberryPi5.step for assembly: {e}")
        return None, None

# --- Main Case Creation Function (Refactored) ---

def create_case():
    """Generates the 3D model of the Pi 5 stacked behind the Touchscreen case using helper functions."""

    # 1. Create Screen Cutout Shape
    cutout_shape = create_screen_cutout_shape(lcd_base_pos_x, lcd_base_pos_y, lcd, external_height)
    if not cutout_shape: return None

    # 2. Create Outer Box and Apply Cutout
    outer_box_cut = create_outer_box_with_cutout(external_width, external_depth, external_height, cutout_shape)
    if not outer_box_cut: return None # If creation/cut failed

    # 3. Hollow out the shell
    case_shell = create_hollow_shell(outer_box_cut, external_width, external_depth, external_height, wall_thickness, base_thickness)
    if not case_shell: return None

    # 4. Calculate Standoff Positions
    absolute_inner_positions = [(lcd_base_pos_x + x, lcd_base_pos_y + y) for x, y in lcd.inner_mount_hole_positions_relative()]
    relative_outer_positions = lcd.outer_mount_hole_positions_relative_lcd(inset=LCD_OUTER_HOLE_INSET)
    absolute_outer_positions = [(lcd_base_pos_x + x, lcd_base_pos_y + y) for x, y in relative_outer_positions]

    # 5. Create Outer Standoffs (Starting from LCD Top) - Inner standoffs removed for direct Pi contact
    # 5. Create Inner and Outer Standoffs (Starting from LCD Top)
    inner_standoffs = create_standoffs(lcd_top_z, absolute_inner_positions, standoff_diameter, standoff_height_between)
    outer_standoffs = create_standoffs(lcd_top_z, absolute_outer_positions, standoff_diameter, standoff_height_between)
    if not inner_standoffs or not outer_standoffs: return None # Standoff creation failed

    # 6. Fuse Shell and Standoffs
    # fused_body_inner = case_shell.fuse(inner_standoffs) # Removed inner standoff fusion
    # if not fused_body_inner:
    #     print("Error: Failed to fuse shell and inner standoffs.")
    #     return None
    fused_body_inner = case_shell.fuse(inner_standoffs)
    if not fused_body_inner:
        print("Error: Failed to fuse shell and inner standoffs.")
        return None
    final_case_body = fused_body_inner.fuse(outer_standoffs)
    if not final_case_body:
        print("Error: Failed to fuse outer standoffs.")
        return None

    # 7. Drill Screw Holes through Standoffs
    all_standoff_positions = absolute_inner_positions + absolute_outer_positions
    screw_holes_shape = drill_screw_holes(
        origin_z=lcd_top_z,
        positions=all_standoff_positions,
        screw_diameter=screw_hole_diameter,
        height=standoff_height_between
    )
    if screw_holes_shape:
        body_with_holes = cq.Workplane(final_case_body).cut(screw_holes_shape).val()
        if not body_with_holes:
            print("Error: Failed to cut screw holes from case body.")
            return None
        final_case_body = body_with_holes # Update final_case_body
    else:
        print("Warning: Skipping screw hole cutting due to creation failure.")
        # Continue without holes if creation failed

    # 8. Fillet Top Edges
    try:
        # Ensure final_case_body is a Workplane object for chaining
        final_case_body_wp = cq.Workplane(final_case_body)
        filleted_body = final_case_body_wp.edges("|Z").fillet(1.5).val()
        if not filleted_body:
             print("Warning: Filleting top edges resulted in an empty shape. Using unfilleted body.")
             # Keep final_case_body as it was before filleting attempt
        else:
             final_case_body = filleted_body # Update final_case_body with filleted version
    except Exception as e:
        print(f"Warning: Could not fillet top edges: {e}. Using unfilleted body.")
        # Keep final_case_body as it was before filleting attempt


    # --- Prepare Assembly Components ---
    lcd_visual_shape, lcd_visual_location = create_lcd_visual(lcd, lcd_base_pos_x, lcd_base_pos_y, lcd_base_pos_z)
    pi_model_shape, pi_model_location = load_pi_model(pi5_base_pos_x, pi5_base_pos_y, pi5_base_pos_z)

    # --- Create Final Assembly ---
    assembly = cq.Assembly()
    # Add the potentially modified final_case_body (with holes and fillets)
    assembly.add(final_case_body, name="case_body", color=cq.Color("lightgray"))
    if lcd_visual_shape:
        assembly.add(lcd_visual_shape, name="lcd_board", color=cq.Color("darkgreen"), loc=lcd_visual_location)
    if pi_model_shape:
        assembly.add(pi_model_shape, name="pi5_model", color=cq.Color("red"), loc=pi_model_location)

    return assembly # Return the assembly object

# --- Main Execution ---

# AI please keep main simple. No try excepts
def main():
    output_filename_base = "eeg_case" # New name for stacked layout

    print("Creating stacked case model...")
    result = create_case() # Call the refactored function

    if result is None:
        print("Case creation failed. Cannot export.")
        return None

    print(f"External Case Dimensions (WxDxH): {external_width / 25.4:.2f} x {external_depth / 25.4:.2f} x {external_height / 25.4:.2f} inches")
    print(f"Internal Case Dimensions (WxDxH): {internal_width / 25.4:.2f} x { internal_depth / 25.4:.2f} x {total_internal_height / 25.4:.2f} inches")
    print(f"Exporting model to {output_filename_base}.step / .stl")

    case_body_shape_to_export = result.objects["case_body"].obj
    cq.exporters.export(case_body_shape_to_export, f"{output_filename_base}.step")
    cq.exporters.export(case_body_shape_to_export, f"{output_filename_base}.stl")

    return result # Return the model/assembly object for Jupyter display

if __name__ == "__main__":
    model = main()
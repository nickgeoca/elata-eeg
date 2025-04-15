import cadquery as cq
from cadquery import importers
import os
import math
from typing import List, Tuple, Optional, Any

# Magic numbers as named constants
ADS1299_VISUAL_X_SHIFT = 200.0
ADS1299_VISUAL_Y_SHIFT = 200.0
LCD_DEFAULT_OUTER_HOLE_INSET = 5.0
LCD_PIN_DIAMETER = 1.0
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

    @classmethod
    def mount_hole_positions_relative(cls):
        """Returns list of (x, y) tuples for mount holes relative to board bottom-left corner."""
        # Based on spacing and offsets
        return [
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y), # Bottom-Left
            (cls.mount_hole_offset_x + cls.mount_hole_spacing_x, cls.mount_hole_offset_y), # Bottom-Right
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y + cls.mount_hole_spacing_y), # Top-Left
            (cls.mount_hole_offset_x + cls.mount_hole_spacing_x, cls.mount_hole_offset_y + cls.mount_hole_spacing_y), # Top-Right
        ]

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
    def outer_mount_hole_positions_relative_lcd(cls, inset: float = LCD_DEFAULT_OUTER_HOLE_INSET) -> List[Tuple[float, float]]:
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

class Ads1299Evm:
    # Dimensions from user input (converted from mils to mm)
    # 1 mil = 0.0254 mm
    board_width = 81.28 # 3200 mils
    board_depth = 88.9 # 3500 mils
    board_thickness = 1.5748 # 62 mils
    pin_height_above_pcb = 6.10 # Height of pin headers above PCB top surface
    component_height_below_pcb = 8.51 # Height of components extending below PCB bottom surface
    total_height_clearance = board_thickness + pin_height_above_pcb + component_height_below_pcb # 16.1848 mm
    height = total_height_clearance # Use calculated total clearance for bounding box / placement
    mount_hole_offset_x = 5.08 # 200 mils from left edge
    mount_hole_offset_y_bottom = 5.08 # 200 mils from bottom edge
    mount_hole_offset_y_top = 5.715 # 225 mils from top edge
    # Calculate spacing based on board depth and edge offsets
    # Note: Provided spacing (3175mil = 80.645mm) differs slightly from calculation (78.105mm).
    # Using calculated value based on edge offsets for positioning consistency.
    mount_hole_spacing_y = board_depth - mount_hole_offset_y_bottom - mount_hole_offset_y_top # 78.105 mm
    mount_hole_diameter = 3.0 # Assuming M3 screws, adjust if needed
    mount_hole_count = 4 # Assuming 4 holes based on typical EVM layout, adjust if needed
    bounding_box = (board_width, board_depth, height)
    # Pinout Dimensions (Positive Channels, converted from mils)
    pin_offset_x = 5.08 # 200 mils from left edge
    pin_base_offset_y = 29.21 # 1150 mils base offset for Ch(n)+ from bottom edge
    pin_spacing_y = 5.08 # 200 mils vertical spacing for Ch(n)+
    bias_pin_offset_y = 29.21 # 1150 mils from bottom edge
    ref_pin_offset_y = 24.13 # 950 mils from bottom edge
    pin_channel_count = 8

    @classmethod
    def mount_hole_positions_relative(cls):
        """Returns list of (x, y) tuples for mount holes relative to board bottom-left corner."""
        # Assuming standard rectangular pattern based on offsets
        # Need confirmation if X spacing is different or if only 2 holes exist
        mount_hole_spacing_x = cls.board_width - 2 * cls.mount_hole_offset_x # Assumes symmetric X placement
        return [
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y_bottom), # Bottom-Left
            (cls.mount_hole_offset_x + mount_hole_spacing_x, cls.mount_hole_offset_y_bottom), # Bottom-Right
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y_bottom + cls.mount_hole_spacing_y), # Top-Left
            (cls.mount_hole_offset_x + mount_hole_spacing_x, cls.mount_hole_offset_y_bottom + cls.mount_hole_spacing_y), # Top-Right
        ]

    @classmethod
    def positive_channel_pin_positions_relative(cls):
        """Returns list of (x, y) tuples for Ch(n)+ pins relative to board bottom-left corner."""
        positions = []
        for n in range(1, cls.pin_channel_count + 1):
            y_pos = cls.pin_base_offset_y + n * cls.pin_spacing_y
            positions.append((cls.pin_offset_x, y_pos))
        return positions
# --- Instantiate Components ---
pi5 = Pi5()
lcd = LCD()
ads1299_evm = Ads1299Evm() # Instance created, though not used yet

# --- Case Configuration ---
wall_thickness = 2.0
base_thickness = 2.0
# total_internal_height = 15.0 # OLD - Now calculated dynamically
standoff_height_base = 2.0 # Height of standoffs from the case base to the bottom of the first component (LCD)
LCD_MOUNT_HEIGHT = 5.0 # Height of the LCD's own integrated mounting points
BRASS_HEX_STANDOFF_HEIGHT = 4.0 # Height of the add-on brass hex standoff (from M2.5x4+4 spec)
# standoff_height_between = LCD_MOUNT_HEIGHT + BRASS_HEX_STANDOFF_HEIGHT # OLD - Pi is no longer stacked
pi5_standoff_height = 2.0 # Height of standoffs from the case base to the bottom of the Pi 5 PCB
standoff_diameter = 5.0
screw_hole_diameter = 2.7 # For M2.5 screws
counterbore_diameter = 5.0 # For M2.5 screw head
counterbore_depth = 1.5
component_clearance = 2.0 # General clearance around components
top_clearance = 2.0 # Clearance above the top component (Pi 5)
LCD_OUTER_HOLE_INSET = 5.0 # Inset distance from LCD edges for outer mounting/standoff points
SCREEN_CUTOUT_CLEARANCE = 1.0 # Extra space added around the LCD viewable area for the cutout
HEX_STANDOFF_DIAMETER_AF = 4.0 # Diameter Across Flats for M2.5 hex standoff (estimated)
# HEX_STANDOFF_HEIGHT is now BRASS_HEX_STANDOFF_HEIGHT defined above

# --- Calculated Case Dimensions (Stacked Layout) ---
# Internal dimensions accommodate the LARGER of the width/depth footprints
internal_width = max(pi5.width, lcd.board_width) + 2 * component_clearance
internal_depth = max(pi5.depth, lcd.board_depth) + 2 * component_clearance
# Internal height accommodates base standoffs, LCD, gap, Pi, and top clearance
total_internal_height = (
    standoff_height_base
    + lcd.total_depth_clearance # Use LCD clearance height
    # + standoff_height_between # Removed - Pi no longer stacked
    + pi5.height # Estimated height of Pi 5 - Keep for overall height, might need adjustment later
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

# Pi 5 Position (Moved to Base)
# pi5_y_offset_from_bottom_wall = 5.0 # Old offset method
pi5_center_y_target_offset = -150.0 # Target distance for Pi center from bottom internal wall (50 - 200)
pi5_base_pos_x = internal_cavity_min_x + (internal_width - pi5.width) / 2 # Center X in internal cavity
pi5_base_pos_y = internal_cavity_min_y + pi5_center_y_target_offset - (pi5.depth / 2) # Position bottom edge so center is at target offset
pi5_base_pos_z = internal_cavity_min_z + pi5_standoff_height # Pi sits on base standoffs

# --- Helper Functions for Case Creation ---

def create_screen_cutout_shape(
    lcd_base_pos_x: float, lcd_base_pos_y: float, lcd: Any, external_height: float
) -> Optional[Any]:
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

def create_outer_box_with_cutout(
    external_width: float, external_depth: float, external_height: float, screen_cutout_shape: Any
) -> Any:
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

def create_hollow_shell(
    box_with_cutout: Any,
    external_width: float,
    external_depth: float,
    external_height: float,
    wall_thickness: float,
    base_thickness: float,
) -> Optional[Any]:
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

def create_standoffs(
    origin_z: float, positions: List[Tuple[float, float]], diameter: float, height: float
) -> Optional[Any]:
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
def drill_screw_holes(
    origin_z: float, positions: List[Tuple[float, float]], screw_diameter: float, height: float
) -> Optional[Any]:
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

def create_hex_standoffs(
    origin_z: float, positions: List[Tuple[float, float]], diameter_across_flats: float, height: float
) -> Optional[Any]:
    """Creates a set of hexagonal standoffs at the given positions."""
    # CadQuery polygon needs diameter across vertices
    diameter_across_vertices = diameter_across_flats / math.cos(math.pi / 6) # pi/6 rad = 30 deg

    shape = (
        cq.Workplane("XY", origin=(0, 0, origin_z))
        .pushPoints(positions)
        .polygon(6, diameter_across_vertices) # 6 sides
        .extrude(height)
    ).val()
    if not shape:
        print(f"Error: Failed to create hex standoffs starting at Z={origin_z}.")
    return shape


def create_lcd_visual(
    lcd: Any, lcd_base_pos_x: float, lcd_base_pos_y: float, lcd_base_pos_z: float
) -> Tuple[Optional[Any], Optional[Any]]:
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

def create_ads1299_visual(
    ads: Any, base_pos_x: float, base_pos_y: float, base_pos_z: float
) -> Tuple[Optional[Any], Optional[Any]]:
    """Creates the visual representation of the ADS1299 EVM board, including pins."""
    pin_diameter = LCD_PIN_DIAMETER

    # --- Create PCB ---
    pcb_wp = cq.Workplane("XY").box(
        ads.board_width, ads.board_depth, ads.board_thickness, centered=(True, True, True)
    )
    pcb_shape = pcb_wp.val()
    if not pcb_shape:
        print("Error: Failed to create base ADS1299 board shape.")
        return None, None

    # --- Create Pins ---
    pin_shapes = cq.Workplane("XY")
    ch_pin_positions_rel_bl = ads.positive_channel_pin_positions_relative()
    bias_pin_pos_rel_bl = (ads.pin_offset_x, ads.bias_pin_offset_y)
    ref_pin_pos_rel_bl = (ads.pin_offset_x, ads.ref_pin_offset_y)
    all_pin_positions_rel_bl = ch_pin_positions_rel_bl + [bias_pin_pos_rel_bl, ref_pin_pos_rel_bl]

    pcb_top_z = ads.board_thickness / 2
    pin_start_z = pcb_top_z
    pin_extrusion_height = ads.pin_height_above_pcb

    pin_points_center_relative = [
        (x_rel_bl - (ads.board_width / 2), y_rel_bl - (ads.board_depth / 2))
        for x_rel_bl, y_rel_bl in all_pin_positions_rel_bl
    ]

    pins = (
        pin_shapes.workplane(offset=pin_start_z)
        .pushPoints(pin_points_center_relative)
        .circle(pin_diameter / 2)
        .extrude(pin_extrusion_height)
    ).val()

    if pins:
        combined_shape = pcb_shape.fuse(pins)
        if not combined_shape:
            print("Warning: Failed to fuse pins to ADS1299 PCB visual. Showing PCB only.")
            combined_shape = pcb_shape
    else:
        print("Warning: Failed to create pin visuals for ADS1299.")
        combined_shape = pcb_shape

    rotated_shape = combined_shape.rotate((0,0,0), (1,0,0), 180)
    if not rotated_shape:
        print("Warning: Failed to rotate ADS1299 visual. Using original orientation.")
        rotated_shape = combined_shape

    rotated_bb = rotated_shape.BoundingBox()
    center_offset_z = -rotated_bb.zmin
    final_center_x = base_pos_x + ads.board_width / 2
    final_center_y = base_pos_y + ads.board_depth / 2
    final_center_z = base_pos_z + center_offset_z

    location = cq.Location(cq.Vector(final_center_x, final_center_y, final_center_z))

    return rotated_shape, location

def load_pi_model(
    pi5_base_pos_x: float, pi5_base_pos_y: float, pi5_base_pos_z: float
) -> Tuple[Optional[Any], Optional[Any]]:
    """Loads, rotates, and positions the Raspberry Pi STEP model."""
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
    # 2. Rotate -90 degrees around Z-axis to orient ports towards the 'right' (positive X)
    pi5_rotated = pi5_rotated_flat.rotate((0, 0, 0), (0, 0, 1), -90)
    pi5_model_shape = pi5_rotated.val()
    if not pi5_model_shape:
        print(f"Warning: Rotated RaspberryPi5.step resulted in an empty shape.")
        return None, None

    # Calculate the placement position based on the model's bounding box
    pi5_bb = pi5_model_shape.BoundingBox()
    pi_place_x = pi5_base_pos_x - pi5_bb.xmin
    pi_place_y = pi5_base_pos_y - pi5_bb.ymin
    # Adjust Z placement to align PCB bottom, not lowest component
    pi_place_z = pi5_base_pos_z - (pi5_bb.zmin + pi5.underside_clearance)
    pi5_model_location = cq.Location(cq.Vector(pi_place_x, pi_place_y, pi_place_z))
    print(f"Successfully loaded and prepared RaspberryPi5.step")
    return pi5_model_shape, pi5_model_location

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
    # 5. Create LCD's Integrated Mount Points (Fused) and Add-on Brass Hex Standoffs (Separate)
    # Create 5mm cylindrical mounts fused to the case for ALL points first
    inner_lcd_mounts = create_standoffs(lcd_top_z, absolute_inner_positions, standoff_diameter, LCD_MOUNT_HEIGHT)
    outer_lcd_mounts = create_standoffs(lcd_top_z, absolute_outer_positions, standoff_diameter, LCD_MOUNT_HEIGHT)
    # Create 5mm brass hex standoffs for inner points, starting ON TOP of the LCD mounts - REMOVED
    # brass_hex_standoffs_shape = create_hex_standoffs(lcd_top_z + LCD_MOUNT_HEIGHT, absolute_inner_positions, HEX_STANDOFF_DIAMETER_AF, BRASS_HEX_STANDOFF_HEIGHT)
    if not inner_lcd_mounts or not outer_lcd_mounts: return None # Creation failed (Removed brass check)

    # 6. Fuse Shell and Standoffs
    # fused_body_inner = case_shell.fuse(inner_standoffs) # Removed inner standoff fusion
    # if not fused_body_inner:
    #     print("Error: Failed to fuse shell and inner standoffs.")
    #     return None
    # 6. Fuse LCD Mount Points (Inner and Outer) to Shell AND Add Pi 5 Base Standoffs
    fused_body_inner = case_shell.fuse(inner_lcd_mounts)
    if not fused_body_inner:
        print("Error: Failed to fuse inner LCD mounts.")
        return None
    final_case_body = fused_body_inner.fuse(outer_lcd_mounts)
    if not final_case_body:
        print("Error: Failed to fuse outer LCD mounts.")
        return None

    # 7. Drill Screw Holes through Standoffs
    # 7. Drill Screw Holes through the fused LCD Mount Points (Inner and Outer)
    all_lcd_mount_positions = absolute_inner_positions + absolute_outer_positions
    screw_holes_shape = drill_screw_holes(
        origin_z=lcd_top_z, # Holes start from the LCD plane
        positions=all_lcd_mount_positions,
        screw_diameter=screw_hole_diameter,
        height=LCD_MOUNT_HEIGHT # Drill through the 5mm fused mounts
    )
    # --- Add Pi 5 Base Standoffs ---
    pi5_mount_positions_rel = pi5.mount_hole_positions_relative()
    absolute_pi5_positions = [(pi5_base_pos_x + x, pi5_base_pos_y + y) for x, y in pi5_mount_positions_rel]
    pi5_base_standoffs = create_standoffs(
        origin_z=internal_cavity_min_z, # Start from case base inner surface
        positions=absolute_pi5_positions,
        diameter=standoff_diameter,
        height=pi5_standoff_height
    )
    if pi5_base_standoffs:
        final_case_body = final_case_body.fuse(pi5_base_standoffs) # Fuse the shapes directly
        if not final_case_body:
            print("Error: Failed to fuse Pi 5 base standoffs.")
            return None
    else:
        print("Error: Failed to create Pi 5 base standoffs.")
        return None

    # 7. Drill Screw Holes (LCD Mounts)
    if screw_holes_shape:
        body_with_lcd_holes = cq.Workplane(final_case_body).cut(screw_holes_shape).val()
        if not body_with_lcd_holes:
            print("Error: Failed to cut LCD screw holes from case body.")
            # Don't return None, maybe Pi holes will work
        else:
            final_case_body = body_with_lcd_holes # Update final_case_body
    else:
        print("Warning: Skipping LCD screw hole cutting due to creation failure.")

    # --- Drill Pi 5 Screw Holes (from bottom) ---
    pi5_screw_hole_depth = base_thickness + pi5_standoff_height # Drill through base and standoff
    # Create the holes starting from the bottom face of the existing body
    body_with_pi_holes_wp = (
        cq.Workplane(final_case_body) # Start with the current case body
        .faces("<Z") # Select the bottom face (minimum Z)
        .workplane() # Create a workplane on that face
        .pushPoints(absolute_pi5_positions) # Define hole centers relative to the face origin
        .cboreHole(screw_hole_diameter, counterbore_diameter, counterbore_depth, depth=pi5_screw_hole_depth)
    )
    # The result of the cboreHole operation is the modified solid
    final_case_body_with_pi_holes = body_with_pi_holes_wp.val() # Get the resulting solid shape

    # Replace the old cut logic with a check on the result
    # pi5_screw_holes_shape = pi5_screw_holes_wp.val() # Old way

    if final_case_body_with_pi_holes:
        final_case_body = final_case_body_with_pi_holes # Update final_case_body with the version containing Pi holes
    else:
         print("Error: Failed to create Pi 5 screw holes using cboreHole.")
         # Don't return None, proceed with potentially partial holes


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

    # --- Create ADS1299 Visual ---
    # Place it next to the main case for visualization
    ads_base_pos_x = external_width / 2 + component_clearance - 180.0 # Place right of the case, then shift left 180mm (moved 20mm to the right)
    ads_base_pos_y = -ads1299_evm.board_depth / 2 - 200.0 # Center it vertically relative to origin, then shift down 200mm
    ads_base_pos_z = 0 # Place it on the base plane (Z=0)
    ads_visual_shape, ads_visual_location = create_ads1299_visual(ads1299_evm, ads_base_pos_x, ads_base_pos_y, ads_base_pos_z)

    # --- Create Final Assembly ---
    assembly = cq.Assembly()
    # Add the potentially modified final_case_body (with holes and fillets)
    assembly.add(final_case_body, name="case_body", color=cq.Color("lightgray"))
    # Add the brass hex standoffs (sitting on top of inner LCD mounts) as separate components - REMOVED
    # if 'brass_hex_standoffs_shape' in locals() and brass_hex_standoffs_shape:
    #     assembly.add(brass_hex_standoffs_shape, name="brass_hex_standoffs", color=cq.Color("goldenrod")) # Brass/Gold color
    if lcd_visual_shape:
        assembly.add(lcd_visual_shape, name="lcd_board", color=cq.Color("darkgreen"), loc=lcd_visual_location)
    if pi_model_shape:
        # Pi position is automatically adjusted as pi5_base_pos_z uses standoff_height_between
        assembly.add(pi_model_shape, name="pi5_model", color=cq.Color("red"), loc=pi_model_location)
    if ads_visual_shape:
        assembly.add(ads_visual_shape, name="ads1299_board", color=cq.Color("blue"), loc=ads_visual_location)

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
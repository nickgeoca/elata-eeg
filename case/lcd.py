import cadquery as cq
from OCP.gp import gp_XYZ # Import gp_XYZ

# (0, 0, 0) is the center of the top face of the LCD board.

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

SCREEN_WIDTH = 109.2 + 0.2  # 109.4 mm
SCREEN_HEIGHT = 66.05 + 0.2  # 66.25 mm
SCREEN_X_OFFSET = 5.65  # from left and right (unused currently, but kept for reference)
SCREEN_Y_OFFSET_TOP = 2.7  # from top

# --- Combined and Transformed Positions Class ---
class TransformedPositions:
    """
    Defines key positions
    """
    def __init__(self, xyzθ=None): 
        self.xyzθ: cq.Location = xyzθ or cq.Location()

    def _to_transformed(self, vecs):
        new_vecs = []
        for vec in vecs:
            point_xyz = gp_XYZ(vec[0], vec[1], vec[2])
            self.xyzθ.wrapped.Transformation().Transforms(point_xyz)
            new_vecs.append(cq.Vector(point_xyz.X(), point_xyz.Y(), point_xyz.Z()))
        return new_vecs

    def update(self, loc) -> cq.Location:
        self.xyzθ = cq.Location(loc) if isinstance(loc, cq.Vector) else loc

    def loc_add(self, vec) -> cq.Location:
        new_loc = cq.Location(self.xyzθ.position() + vec)
        new_loc.wrapped.SetRotation(self.xyzθ.wrapped.Transformation().GetRotation())
        self.xyzθ = new_loc

    # --- Properties return transformed vectors ---
    @property
    def inner_mount_holes(self) -> list[cq.Vector]:
        x_offset = 20  - PCB_WIDTH / 2
        y_bottom = 21.50 - PCB_HEIGHT / 2
        y_top = PCB_HEIGHT - 7.43 - PCB_HEIGHT / 2
        x_spacing = 58.00
        z = -PANEL_THICKNESS - PCB_THICKNESS
        return self._to_transformed([
            (x_offset, y_top, z),  # Top-Left
            (x_offset + x_spacing, y_top, z),  # Top-Right
            (x_offset + x_spacing, y_bottom, z),  # Bottom-Right
            (x_offset, y_bottom, z),  # Bottom-Left
        ])

    @property
    def outer_mount_holes(self) -> list[cq.Vector]:
        # Reduced Form:
        inset = 5.0
        z = -PANEL_THICKNESS - PCB_THICKNESS

        # Calculate half-dimensions and the effective offset from the center
        half_width = PCB_WIDTH / 2
        half_height = PCB_HEIGHT / 2
        x_rel = half_width - inset  # x-distance from center to inset edge
        y_rel = half_height - inset # y-distance from center to inset edge

        # Define points relative to the center (0,0) using the calculated offsets
        return self._to_transformed([
            (-x_rel,  y_rel, z),  # Top-Left
            ( x_rel,  y_rel, z),  # Top-Right
            ( x_rel, -y_rel, z),  # Bottom-Right
            (-x_rel, -y_rel, z),  # Bottom-Left
        ])

    @property
    def top_panel(self) -> list[cq.Vector]:
        # Top of the glass panel corners
        y_top = PCB_HEIGHT / 2
        y_bottom = y_top - PANEL_HEIGHT
        x_left = -PANEL_WIDTH / 2
        x_right = PANEL_WIDTH / 2
        z = 0
        return self._to_transformed([
            (x_left, y_top, z),     # Top-Left
            (x_right, y_top, z),    # Top-Right
            (x_right, y_bottom, z), # Bottom-Right            
            (x_left, y_bottom, z),  # Bottom-Left
        ])

    @property
    def bottom_pcb(self) -> list[cq.Vector]:
        # bottom of the pcb corners
        z = -PANEL_THICKNESS
        return self._to_transformed([
            (PCB_WIDTH / 2, 0, z),     # Top-Left
            (-PCB_WIDTH / 2, 0, z),    # Top-Right
            (-PCB_WIDTH / 2, -PCB_HEIGHT, z), # Bottom-Right            
            (PCB_WIDTH / 2, -PCB_HEIGHT, z),  # Bottom-Left
        ])

_p = TransformedPositions()

def create():
    """
    Generates the LCD assembly (Panel + PCB), adds features, exports a STEP file,
    and returns the CadQuery Assembly and its initial location (identity).
    """
    # Get hole vectors from positions object
    inner_holes = _p.inner_mount_holes
    outer_holes = _p.outer_mount_holes
    all_hole_vectors = inner_holes + outer_holes
    # Calculate required Y offset to align panel top with PCB top
    panel_y_offset = (PCB_HEIGHT / 2) - (PANEL_HEIGHT / 2)

    # --- Create Base Parts ---
    lcd_panel = cq.Workplane("XY").box(
        PANEL_WIDTH, PANEL_HEIGHT, PANEL_THICKNESS, centered=(True, True, False)
    ).translate((0, panel_y_offset, -PANEL_THICKNESS)).val() # Apply Y offset

    pcb = cq.Workplane("XY").box(
        PCB_WIDTH, PCB_HEIGHT, PCB_THICKNESS, centered=(True, True, False)
    ).translate((0, 0, -(PANEL_THICKNESS + PCB_THICKNESS))).val()

    # --- Modify PCB ---
    studs = (
        cq.Workplane("XY", origin=(0, 0, 0)) # Workplane at PCB bottom
        .pushPoints([vec.toTuple() for vec in all_hole_vectors]) # Pass vectors as tuples
        .circle(STUD_DIAMETER / 2)
        .extrude(-STUD_HEIGHT) # Extrude downwards
        .val()
    )
    pcb_with_studs = cq.Workplane(pcb).union(studs).val()
    stud_hole_cylinders = (
        cq.Workplane("XY", origin=(0, 0, -PCB_THICKNESS)) # Workplane at PCB bottom
        .pushPoints([vec.toTuple() for vec in all_hole_vectors]) # Pass vectors as tuples
        .circle(MOUNT_HOLE_DIAMETER / 2)
        .extrude(-STUD_HEIGHT) # Extrude downwards through stud height
    )
    final_pcb = cq.Workplane(pcb_with_studs).cut(stud_hole_cylinders).val()

    # --- Modify Panel ---
    # Calculate screen center position
    screen_center_x = 0  # centered horizontally

    # Calculate screen center Y relative to PCB center (as before)
    pcb_top_y = PCB_HEIGHT / 2
    screen_top_edge_y_rel_pcb_center = pcb_top_y - SCREEN_Y_OFFSET_TOP # Y coord relative to PCB center
    screen_bottom_edge_y_rel_pcb_center = screen_top_edge_y_rel_pcb_center - SCREEN_HEIGHT
    screen_center_y_rel_pcb_center = (screen_top_edge_y_rel_pcb_center + screen_bottom_edge_y_rel_pcb_center) / 2

    screen_center_y_rel_panel_center = screen_center_y_rel_pcb_center - panel_y_offset

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
    assembly.add(lcd_panel, name="lcd_panel", color=cq.Color("darkgreen"))
    assembly.add(final_pcb, name="pcb", color=cq.Color("green"))
    assembly.add(screen_overlay, name="screen_overlay", color=cq.Color("blue"))
    assembly.save('lcd.step')

    return assembly # Only return assembly

if __name__ == "__main__":
    # Call the updated create function and unpack results
    lcd_assembly = create() # Only one return value now

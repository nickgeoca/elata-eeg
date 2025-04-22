import cadquery as cq
import math
from OCP.gp import gp_XYZ # Import gp_XYZ

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

center_offset_x = -BOARD_WIDTH / 2
center_offset_y = -BOARD_DEPTH / 2
pcb_top_z = BOARD_THICKNESS / 2

# --- Combined and Transformed Positions Class ---
class TransformedPositions:
    """
    Defines key positions
    """
    def __init__(self, xyzθ=None):
        self.xyzθ : cq.Location = xyzθ or cq.Location()

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
    def mount_holes(self) -> list[cq.Vector]:
        z = 0
        half_width = BOARD_WIDTH / 2
        half_depth = BOARD_DEPTH / 2
        x_left = -half_width + MOUNT_HOLE_OFFSET_X
        y_bottom = -half_depth + MOUNT_HOLE_OFFSET_Y_BOTTOM
        y_top = half_depth - MOUNT_HOLE_OFFSET_Y_TOP
        
        # Define points relative to the center (0,0) using the calculated offsets
        return self._to_transformed([
            (x_left, y_bottom, z),  # Bottom-Left
            (x_left, y_top, z),     # Top-Left
        ])

    @property
    def channel_pins(self) -> list[cq.Vector]:
        z = pcb_top_z  # Z at PCB top
        half_width = BOARD_WIDTH / 2
        half_depth = BOARD_DEPTH / 2
        
        # Calculate x position relative to center
        x_pos = -half_width + PIN_OFFSET_X
        
        return self._to_transformed([
            (x_pos, -half_depth + PIN_BASE_OFFSET_Y + n * PIN_SPACING_Y, z) 
         for n in range(1, PIN_CHANNEL_COUNT + 1)])

    @property
    def bias_pin(self) -> cq.Vector:
        z = pcb_top_z  # Z at PCB top
        x_pos = -BOARD_WIDTH / 2 + PIN_OFFSET_X
        y_pos = -BOARD_DEPTH / 2 + BIAS_PIN_OFFSET_Y
        
        # Define point relative to the center (0,0)
        return self._to_transformed([(x_pos, y_pos, z)])[0]

    @property
    def ref_pin(self) -> cq.Vector:
        z = pcb_top_z  # Z at PCB top
        half_width = BOARD_WIDTH / 2
        half_depth = BOARD_DEPTH / 2
        x_pos = -half_width + PIN_OFFSET_X
        y_pos = -half_depth + REF_PIN_OFFSET_Y
        
        # Define point relative to the center (0,0)
        return self._to_transformed([(x_pos, y_pos, z)])[0]


# Create a global instance of TransformedPositions
_p = TransformedPositions()

def create():
    """
    Generates the ADS1299EVM board, exports an unrotated STEP file,
    and returns the CadQuery Assembly.
    """
    all_pin_vectors =  _p.channel_pins + [_p.bias_pin, _p.ref_pin]

    # Create PCB centered at (0,0,0)
    pcb_shape = cq.Workplane("XY").box(
        BOARD_WIDTH, BOARD_DEPTH, BOARD_THICKNESS, centered=(True, True, True)
    ).val()
    if not pcb_shape:
        print("Failed to create ADS1299EVM PCB.")
        return None

    mount_holes = (
        cq.Workplane("XY") # Workplane at Z=0 (mid-plane)
        .pushPoints([vec.toTuple() for vec in _p.mount_holes]) # Pass vectors as tuples
        .circle(MOUNT_HOLE_DIAMETER / 2)
        .extrude(BOARD_THICKNESS * 2, both=True)  # Extrude symmetrically
    )
    pcb_with_holes = cq.Workplane(pcb_shape).cut(mount_holes).val()

    pins = (
        cq.Workplane("XY") # Workplane at Z=0
        .pushPoints([vec.toTuple() for vec in all_pin_vectors]) # Pass vectors as tuples
        .circle(PIN_DIAMETER / 2)
        .extrude(PIN_HEIGHT_ABOVE_PCB)
    ).val()

    combined_shape = pcb_with_holes.fuse(pins)

    assembly = cq.Assembly()
    assembly.add(combined_shape, name="ads1299_board", color=cq.Color("blue"))
    cq.exporters.export(combined_shape, "ads1299evm.step")
    return assembly

if __name__ == "__main__":
    # Call the updated create function and unpack results
    ads_assembly = create() # Only one return value now
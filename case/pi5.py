import cadquery as cq
import os
from OCP.gp import gp_XYZ # Import gp_XYZ

PI5_WIDTH = 85.0
PI5_HEIGHT = 56.0
PI5_THICKNESS = 17.0 # Approximate overall thickness

# --- Combined and Transformed Positions Class ---
class TransformedPositions:
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

    def loc_add(self, vec_to_add: cq.Vector) -> cq.Location:
        from OCP.gp import gp_Trsf, gp_Vec, gp_Quaternion # Import necessary OCP types
        current_trsf: gp_Trsf = self.xyzθ.wrapped.Transformation()
        current_rot: gp_Quaternion = current_trsf.GetRotation()
        current_trans_part: gp_Vec = current_trsf.TranslationPart()
        new_trans_vec = gp_Vec(
            current_trans_part.X() + vec_to_add.x,
            current_trans_part.Y() + vec_to_add.y,
            current_trans_part.Z() + vec_to_add.z
        )
        new_trsf = gp_Trsf()
        new_trsf.SetRotation(current_rot)
        new_trsf.SetTranslationPart(new_trans_vec)
        new_loc = cq.Location(new_trsf)
        self.xyzθ = new_loc
        return self.xyzθ
    def loc_set(self, new_vec: cq.Vector) -> cq.Location:
        from OCP.gp import gp_Trsf, gp_Vec, gp_Quaternion # Import necessary OCP types
        current_trsf: gp_Trsf = self.xyzθ.wrapped.Transformation()
        current_rot: gp_Quaternion = current_trsf.GetRotation()
        new_trans_vec = gp_Vec(new_vec.x, new_vec.y, new_vec.z)
        new_trsf = gp_Trsf()
        new_trsf.SetRotation(current_rot)
        new_trsf.SetTranslationPart(new_trans_vec)
        new_loc = cq.Location(new_trsf)
        self.xyzθ = new_loc
        return self.xyzθ
    
    @property
    def mounting_holes(self) -> list[cq.Vector]:
        # Original positions relative to center origin (Y+ up)
        hole_offset_y = 49.0 / 2
        return self._to_transformed([
            (-39, -hole_offset_y, 0), # Bottom-Left
            ( 19, -hole_offset_y, 0), # Bottom-Right
            (-39,  hole_offset_y, 0), # Top-Left
            ( 19,  hole_offset_y, 0), # Top-Right
        ])


    @property
    def pcb_bottom(self) -> cq.Vector:
        # Original position
        return self._to_transformed([(0, 0, 0)])[0]

    @property
    def pcb_top(self) -> cq.Vector:
        # Original position calculation
        thickness = 1.4
        return self._to_transformed([(0, thickness, 0)])[0]

# Create a global instance of TransformedPositions
_p = TransformedPositions()

def create():
    """
    Loads the Raspberry Pi 5 STEP model and returns the CadQuery Assembly.
    The global TransformedPositions instance (_p) is used to access key positions.
    """
    pi5_model = cq.importers.importStep(os.path.join(os.path.dirname(__file__), "RaspberryPi5.step"))
    pi5_shape = pi5_model.val()
    assembly = cq.Assembly()
    assembly.add(pi5_shape, name="raspberry_pi_5", color=cq.Color("red"))
    return assembly

if __name__ == "__main__":
    pi5_assembly = create()
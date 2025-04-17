import cadquery as cq

from config import (
    Pi5,
    LCD,
    Ads1299Evm,
    wall_thickness,
    base_thickness,
    standoff_height_base,
    LCD_MOUNT_HEIGHT,
    pi5_standoff_height,
    standoff_diameter,
    screw_hole_diameter,
    counterbore_diameter,
    counterbore_depth,
    component_clearance,
    top_clearance,
    LCD_OUTER_HOLE_INSET,
)
from .geometry import (
    create_screen_cutout_shape,
    create_outer_box_with_cutout,
    create_hollow_shell,
    create_standoffs,
    drill_screw_holes,
    create_lcd_visual,
    create_ads1299_visual,
    load_pi_model,
)


class CaseBuilder:
    def __init__(self):
        self.pi5 = Pi5()
        self.lcd = LCD()
        self.ads = Ads1299Evm()

        # internal dimensions
        self.internal_width = max(self.pi5.width, self.lcd.board_width) + 2 * component_clearance
        self.internal_depth = max(self.pi5.depth, self.lcd.board_depth) + 2 * component_clearance
        # stacked height: base standoffs + LCD + Pi + top clearance
        self.total_internal_height = (
            standoff_height_base
            + self.lcd.total_depth_clearance
            + self.pi5.height
            + top_clearance
        )
        # external dims with walls
        self.external_width = self.internal_width + 2 * wall_thickness
        self.external_depth = self.internal_depth + 2 * wall_thickness
        self.external_height = base_thickness + self.total_internal_height

        # internal cavity origin
        self.internal_cavity_min_x = -self.internal_width / 2
        self.internal_cavity_min_y = -self.internal_depth / 2
        self.internal_cavity_min_z = base_thickness

        # LCD placement
        self.lcd_base_pos_x = self.internal_cavity_min_x + (self.internal_width - self.lcd.board_width) / 2
        self.lcd_base_pos_y = self.internal_cavity_min_y + (self.internal_depth - self.lcd.board_depth) / 2
        self.lcd_base_pos_z = self.internal_cavity_min_z + standoff_height_base
        self.lcd_top_z = self.lcd_base_pos_z + self.lcd.board_thickness

        # Pi5 placement
        self.pi5_base_pos_x = self.internal_cavity_min_x + (self.internal_width - self.pi5.width) / 2
        center_offset = -150.0  # target offset tweak
        self.pi5_base_pos_y = (
            self.internal_cavity_min_y + center_offset - (self.pi5.depth / 2) + 100.0
        )
        self.pi5_base_pos_z = self.internal_cavity_min_z + pi5_standoff_height

    def build(self) -> cq.Assembly:
        # 1. cutout
        cut = create_screen_cutout_shape(
            self.lcd_base_pos_x, self.lcd_base_pos_y, self.lcd, self.external_height
        )
        # 2. outer box
        outer = create_outer_box_with_cutout(
            self.external_width, self.external_depth, self.external_height, cut
        )
        # 3. shell
        shell = create_hollow_shell(
            outer,
            self.external_width,
            self.external_depth,
            self.external_height,
            wall_thickness,
            base_thickness,
        )
        # mount positions
        inner_rel = self.lcd.inner_mount_hole_positions_relative()
        abs_inner = [(self.lcd_base_pos_x + x, self.lcd_base_pos_y + y) for x, y in inner_rel]
        outer_rel = self.lcd.outer_mount_hole_positions_relative_lcd(inset=LCD_OUTER_HOLE_INSET)
        abs_outer = [(self.lcd_base_pos_x + x, self.lcd_base_pos_y + y) for x, y in outer_rel]

        # 4. LCD mounts
        inner_mounts = create_standoffs(self.lcd_top_z, abs_inner, standoff_diameter, LCD_MOUNT_HEIGHT)
        outer_mounts = create_standoffs(self.lcd_top_z, abs_outer, standoff_diameter, LCD_MOUNT_HEIGHT)
        body = shell.fuse(inner_mounts).fuse(outer_mounts)

        # 5. Pi5 standoffs
        pi_positions = self.pi5.mount_hole_positions_relative()
        abs_pi = [(self.pi5_base_pos_x + x, self.pi5_base_pos_y + y) for x, y in pi_positions]
        pi_stand = create_standoffs(
            self.internal_cavity_min_z, abs_pi, standoff_diameter, pi5_standoff_height
        )
        body = body.fuse(pi_stand)

        # 6. LCD screw holes
        holes_lcd = drill_screw_holes(
            self.lcd_top_z, abs_inner + abs_outer, screw_hole_diameter, LCD_MOUNT_HEIGHT
        )
        if holes_lcd:
            body = cq.Workplane(body).cut(holes_lcd).val()

        # 7. Pi5 counterbore holes
        depth = base_thickness + pi5_standoff_height
        body = (
            cq.Workplane(body)
            .faces("<Z")
            .workplane()
            .pushPoints(abs_pi)
            .cboreHole(screw_hole_diameter, counterbore_diameter, counterbore_depth, depth=depth)
            .val()
        )

        # 8. fillet top edges
        try:
            fil = cq.Workplane(body).edges("|Z").fillet(1.5).val()
            if fil:
                body = fil
        except Exception:
            pass

        # visuals and assembly
        lcd_shape, lcd_loc = create_lcd_visual(
            self.lcd, self.lcd_base_pos_x, self.lcd_base_pos_y, self.lcd_base_pos_z
        )
        pi_shape, pi_loc = load_pi_model(
            self.pi5_base_pos_x, self.pi5_base_pos_y, self.pi5_base_pos_z
        )
        ads_shape, ads_loc = create_ads1299_visual(
            self.ads,
            self.external_width / 2 + component_clearance - 130.0,
            -self.ads.board_depth / 2 - 200.0 + 50.0,
            0,
        )

        asm = cq.Assembly()
        asm.add(body, name="case_body", color=cq.Color("lightgray"))
        if lcd_shape:
            asm.add(lcd_shape, name="lcd_board", color=cq.Color("darkgreen"), loc=lcd_loc)
        if pi_shape:
            asm.add(pi_shape, name="pi5_model", color=cq.Color("red"), loc=pi_loc)
        if ads_shape:
            asm.add(ads_shape, name="ads1299_board", color=cq.Color("blue"), loc=ads_loc)

        return asm


def create_case() -> cq.Assembly:
    return CaseBuilder().build()
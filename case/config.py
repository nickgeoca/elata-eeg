from typing import List, Tuple

# Magic numbers as named constants
ADS1299_VISUAL_X_SHIFT = 200.0
ADS1299_VISUAL_Y_SHIFT = 200.0
LCD_DEFAULT_OUTER_HOLE_INSET = 5.0
LCD_PIN_DIAMETER = 1.0

class Pi5:
    width: float = 85.0
    depth: float = 56.0
    height: float = 20.0
    mount_hole_spacing_x: float = 58.0
    mount_hole_spacing_y: float = 49.0
    mount_hole_offset_x: float = 3.5
    mount_hole_offset_y: float = (depth - mount_hole_spacing_y) / 2
    bounding_box: Tuple[float, float, float] = (width, depth, height)
    underside_clearance: float = 2.0

    @classmethod
    def mount_hole_positions_relative(cls) -> List[Tuple[float, float]]:
        return [
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y),
            (cls.mount_hole_offset_x + cls.mount_hole_spacing_x, cls.mount_hole_offset_y),
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y + cls.mount_hole_spacing_y),
            (cls.mount_hole_offset_x + cls.mount_hole_spacing_x, cls.mount_hole_offset_y + cls.mount_hole_spacing_y),
        ]

class LCD:
    board_width: float = 121.11
    board_depth: float = 77.93
    board_thickness: float = 2.0
    total_depth_clearance: float = 12.0
    va_width: float = 109.20
    va_depth: float = 66.05
    mount_hole_diameter: float = 2.5
    inner_x_offset: float = 43.11
    inner_y_offset_bottom: float = 21.50
    inner_y_offset_top: float = board_depth - 7.43
    inner_x_spacing: float = 58.00
    bounding_box: Tuple[float, float, float] = (board_width, board_depth, total_depth_clearance)

    @classmethod
    def inner_mount_hole_positions_relative(cls) -> List[Tuple[float, float]]:
        return [
            (cls.inner_x_offset, cls.inner_y_offset_top),
            (cls.inner_x_offset + cls.inner_x_spacing, cls.inner_y_offset_top),
            (cls.inner_x_offset, cls.inner_y_offset_bottom),
            (cls.inner_x_offset + cls.inner_x_spacing, cls.inner_y_offset_bottom),
        ]

    @classmethod
    def outer_mount_hole_positions_relative_lcd(cls, inset: float = LCD_DEFAULT_OUTER_HOLE_INSET) -> List[Tuple[float, float]]:
        return [
            (inset, inset),
            (cls.board_width - inset, inset),
            (inset, cls.board_depth - inset),
            (cls.board_width - inset, cls.board_depth - inset),
        ]

class Ads1299Evm:
    board_width: float = 81.28
    board_depth: float = 88.9
    board_thickness: float = 1.5748
    pin_height_above_pcb: float = 6.10
    component_height_below_pcb: float = 8.51
    total_height_clearance: float = board_thickness + pin_height_above_pcb + component_height_below_pcb
    height: float = total_height_clearance
    mount_hole_offset_x: float = 5.08
    mount_hole_offset_y_bottom: float = 5.08
    mount_hole_offset_y_top: float = 5.715
    mount_hole_spacing_y: float = board_depth - mount_hole_offset_y_bottom - mount_hole_offset_y_top
    mount_hole_diameter: float = 3.0
    mount_hole_count: int = 4
    bounding_box: Tuple[float, float, float] = (board_width, board_depth, height)
    pin_offset_x: float = 5.08
    pin_base_offset_y: float = 29.21
    pin_spacing_y: float = 5.08
    bias_pin_offset_y: float = 29.21
    ref_pin_offset_y: float = 24.13
    pin_channel_count: int = 8

    @classmethod
    def mount_hole_positions_relative(cls) -> List[Tuple[float, float]]:
        mount_hole_spacing_x = cls.board_width - 2 * cls.mount_hole_offset_x
        return [
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y_bottom),
            (cls.mount_hole_offset_x + mount_hole_spacing_x, cls.mount_hole_offset_y_bottom),
            (cls.mount_hole_offset_x, cls.mount_hole_offset_y_bottom + cls.mount_hole_spacing_y),
            (cls.mount_hole_offset_x + mount_hole_spacing_x, cls.mount_hole_offset_y_bottom + cls.mount_hole_spacing_y),
        ]

    @classmethod
    def positive_channel_pin_positions_relative(cls) -> List[Tuple[float, float]]:
        positions = []
        for n in range(1, cls.pin_channel_count + 1):
            y_pos = cls.pin_base_offset_y + n * cls.pin_spacing_y
            positions.append((cls.pin_offset_x, y_pos))
        return positions

# Case configuration constants
wall_thickness: float = 2.0
base_thickness: float = 2.0
standoff_height_base: float = 2.0
LCD_MOUNT_HEIGHT: float = 5.0
BRASS_HEX_STANDOFF_HEIGHT: float = 4.0
pi5_standoff_height: float = 2.0
standoff_diameter: float = 5.0
screw_hole_diameter: float = 2.7
counterbore_diameter: float = 5.0
counterbore_depth: float = 1.5
component_clearance: float = 2.0
top_clearance: float = 2.0
LCD_OUTER_HOLE_INSET: float = 5.0
SCREEN_CUTOUT_CLEARANCE: float = 1.0
HEX_STANDOFF_DIAMETER_AF: float = 4.0
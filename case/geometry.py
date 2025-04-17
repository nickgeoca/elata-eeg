import os
import math
from typing import Any, List, Tuple, Optional

import cadquery as cq
from cadquery import importers

from .config import (
    ADS1299_VISUAL_X_SHIFT,
    ADS1299_VISUAL_Y_SHIFT,
    LCD_DEFAULT_OUTER_HOLE_INSET,
    LCD_PIN_DIAMETER,
    SCREEN_CUTOUT_CLEARANCE,
    Pi5,
    LCD,
    Ads1299Evm,
)

def create_screen_cutout_shape(
    lcd_base_pos_x: float,
    lcd_base_pos_y: float,
    lcd: LCD,
    external_height: float,
) -> Optional[Any]:
    lcd_center_x = lcd_base_pos_x + lcd.board_width / 2
    lcd_center_y = lcd_base_pos_y + lcd.board_depth / 2
    cutout_width = lcd.va_width + SCREEN_CUTOUT_CLEARANCE
    cutout_depth = lcd.va_depth + SCREEN_CUTOUT_CLEARANCE
    shape = (
        cq.Workplane("XY", origin=(0, 0, external_height))
        .center(lcd_center_x, lcd_center_y)
        .rect(cutout_width, cutout_depth)
        .extrude(-external_height)
    ).val()
    return shape

def create_outer_box_with_cutout(
    external_width: float,
    external_depth: float,
    external_height: float,
    screen_cutout_shape: Any,
) -> Any:
    outer_box = cq.Workplane("XY").box(
        external_width, external_depth, external_height, centered=(True, True, False)
    )
    if screen_cutout_shape:
        result = outer_box.cut(screen_cutout_shape)
        return result.val()
    return outer_box.val()

def create_hollow_shell(
    box_with_cutout: Any,
    external_width: float,
    external_depth: float,
    external_height: float,
    wall_thickness: float,
    base_thickness: float,
) -> Optional[Any]:
    inner_w = external_width - 2 * wall_thickness
    inner_d = external_depth - 2 * wall_thickness
    inner_h = external_height - base_thickness
    inner_box = (
        cq.Workplane("XY")
        .box(inner_w, inner_d, inner_h, centered=(True, True, False))
        .translate((0, 0, base_thickness))
    )
    shell = cq.Workplane(box_with_cutout).cut(inner_box).val()
    return shell

def create_standoffs(
    origin_z: float,
    positions: List[Tuple[float, float]],
    diameter: float,
    height: float,
) -> Optional[Any]:
    pts = [(x, y) for x, y in positions]
    shape = (
        cq.Workplane("XY", origin=(0, 0, origin_z))
        .pushPoints(pts)
        .circle(diameter / 2)
        .extrude(height)
    ).val()
    return shape

def drill_screw_holes(
    origin_z: float,
    positions: List[Tuple[float, float]],
    screw_diameter: float,
    height: float,
) -> Optional[Any]:
    hole_cyl = (
        cq.Workplane("XY", origin=(0, 0, origin_z))
        .pushPoints(positions)
        .circle(screw_diameter / 2)
        .extrude(height)
    )
    return hole_cyl.val()

def create_hex_standoffs(
    origin_z: float,
    positions: List[Tuple[float, float]],
    diameter_across_flats: float,
    height: float,
) -> Optional[Any]:
    dia_vertices = diameter_across_flats / math.cos(math.pi / 6)
    shape = (
        cq.Workplane("XY", origin=(0, 0, origin_z))
        .pushPoints(positions)
        .polygon(6, dia_vertices)
        .extrude(height)
    ).val()
    return shape

def create_lcd_visual(
    lcd: LCD,
    lcd_base_pos_x: float,
    lcd_base_pos_y: float,
    lcd_base_pos_z: float,
) -> Tuple[Optional[Any], Optional[Any]]:
    shape = cq.Workplane("XY").box(
        lcd.board_width, lcd.board_depth, lcd.board_thickness, centered=(True, True, False)
    ).val()
    vis_pos_x = lcd_base_pos_x + lcd.board_width / 2
    vis_pos_y = lcd_base_pos_y + lcd.board_depth / 2
    location = cq.Location(cq.Vector(vis_pos_x, vis_pos_y, lcd_base_pos_z))
    return shape, location

def create_ads1299_visual(
    ads: Ads1299Evm,
    base_pos_x: float,
    base_pos_y: float,
    base_pos_z: float,
) -> Tuple[Optional[Any], Optional[Any]]:
    pcb_wp = cq.Workplane("XY").box(
        ads.board_width, ads.board_depth, ads.board_thickness, centered=(True, True, True)
    )
    pcb = pcb_wp.val()
    ch_positions = ads.positive_channel_pin_positions_relative()
    bias = (ads.pin_offset_x, ads.bias_pin_offset_y)
    ref = (ads.pin_offset_x, ads.ref_pin_offset_y)
    all_pts = ch_positions + [bias, ref]
    center_rel = [
        (x - ads.board_width / 2, y - ads.board_depth / 2) for x, y in all_pts
    ]
    pin_start = ads.board_thickness / 2
    pins = (
        cq.Workplane("XY")
        .workplane(offset=pin_start)
        .pushPoints(center_rel)
        .circle(LCD_PIN_DIAMETER / 2)
        .extrude(ads.pin_height_above_pcb)
    ).val()
    combined = pcb.fuse(pins) if pins else pcb
    rotated = combined.rotate((0,0,0), (1,0,0), 180).rotate((0,0,0), (0,0,1), 90)
    bb = rotated.BoundingBox()
    offset_z = -bb.zmin
    loc = cq.Location(cq.Vector(
        base_pos_x + ads.board_width/2,
        base_pos_y + ads.board_depth/2,
        base_pos_z + offset_z
    ))
    return rotated, loc

def load_pi_model(
    pi5_base_pos_x: float,
    pi5_base_pos_y: float,
    pi5_base_pos_z: float,
) -> Tuple[Optional[Any], Optional[Any]]:
    step_path = os.path.join(os.path.dirname(__file__), "RaspberryPi5.step")
    if not os.path.exists(step_path):
        return None, None
    imported = importers.importStep(step_path)
    shape = imported.val() if imported else None
    if not shape:
        return None, None
    rot_x = shape.rotate((0,0,0), (1,0,0), -90)
    rot_z = rot_x.rotate((0,0,0), (0,0,1), -90).val()
    bb = rot_z.BoundingBox()
    loc = cq.Location(cq.Vector(
        pi5_base_pos_x - bb.xmin,
        pi5_base_pos_y - bb.ymin,
        pi5_base_pos_z - (bb.zmin + Pi5.underside_clearance),
    ))
    return rot_z, loc
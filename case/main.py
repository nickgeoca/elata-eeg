import cadquery as cq
import pi5 as pi5
import lcd as lcd
import ads1299evm as ads

# Parameters
case_thickness = 3.0

def main():
    pi5_pos = pi5.TransformedPositions()
    lcd_pos = lcd.TransformedPositions()
    ads_pos = ads.TransformedPositions()
    # <---- Move and line up the components ----> #

    mount_pi5_to_lcd_and_rotate = (cq.Vector(-1.5, 7, -10), cq.Vector(1, 0, 0), -90)
    pi5_pos.update(cq.Location(*mount_pi5_to_lcd_and_rotate))

    position_adc = (cq.Vector(0, -90, -20), cq.Vector(0, 0, 1), 90)
    ads_pos.update(cq.Location(*position_adc))

    # Create LCD top glass outline
    # Parameters
    lip_overlap = 1.5
    z_clearance_above_glass = 1.5
    lcd_top_points = lcd_pos.top_panel
    lcd_top = cq.Workplane("XY").polyline([(v.x, v.y) for v in lcd_top_points]).close()

    # Outer case perimeter = LCD glass + lip overlap
    outer_case = lcd_top.offset2D(lip_overlap)

    # Case wall (raised lip) around glass
    lcd_lip = (
        cq.Workplane("XY")
        .center(0, 0)
        .polyline(outer_case.val().Vertices())  # Outer contour
        .close()
        .extrude(z_clearance_above_glass)
    )

    # Case base below glass
    lcd_base = (
        cq.Workplane("XY")
        .polyline(outer_case.val().Vertices())  # Use same outer offset
        .close()
        .extrude(-case_thickness)
    )

    # Combine base and lip
    lcd_case = lcd_base.union(lcd_lip)

    # Translate case to align with LCD position
    lcd_case = lcd_case.translate(lcd_pos.xyzθ.toTuple()[0])  # Assuming lcd_pos is a cq.Location



    # <----            Assemble             ----> #
    main_assembly = cq.Assembly()
    main_assembly.add(pi5.create(), name="pi5_instance", loc=pi5_pos.xyzθ)
    main_assembly.add(lcd.create(), name="lcd_instance", loc=lcd_pos.xyzθ)
    main_assembly.add(ads.create(), name="ads_instance", loc=ads_pos.xyzθ)
    main_assembly.add(lcd_case, name="lcd_case")

    print(f"  FINAL Pi 5 xyz: { pi5_pos.xyzθ.toTuple()[0]}")
    print(f"  FINAL ADS1299evm xyz: { ads_pos.xyzθ.toTuple()[0]}")
    print(f"  FINAL LCD xyz: { lcd_pos.xyzθ.toTuple()[0]}")
    return main_assembly
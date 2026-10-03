//! ARGB pixmaps for the tray. No theme icons, so the state is visible on any
//! Plasma panel.

use ksni::Icon;

use crate::tray::TrayIcon;

const SIZE: usize = 32;
const SIZE_PX: i32 = 32;

/// A 32×32 ARGB pixmap (alpha, red, green, blue), the order ksni expects.
#[must_use]
pub fn pixmap(icon: TrayIcon) -> Icon {
    let mut data = vec![0; SIZE * SIZE * 4];
    let (red, green, blue) = icon.rgb();
    let ring = matches!(icon, TrayIcon::Down);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = i32::try_from(x).unwrap_or(0) - SIZE_PX / 2;
            let dy = i32::try_from(y).unwrap_or(0) - SIZE_PX / 2;
            let dist = dx * dx + dy * dy;
            let ink = if ring {
                (8 * 8..=13 * 13).contains(&dist)
            } else {
                dist <= 13 * 13
            };
            if ink {
                put(&mut data, x, y, [255, red, green, blue]);
            }
        }
    }
    Icon {
        width: SIZE_PX,
        height: SIZE_PX,
        data,
    }
}

fn put(data: &mut [u8], x: usize, y: usize, pixel: [u8; 4]) {
    if x >= SIZE || y >= SIZE {
        return;
    }
    let start = (y * SIZE + x) * 4;
    if let Some(slot) = data.get_mut(start..start + 4) {
        slot.copy_from_slice(&pixel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_its_own_pixmap() {
        let pixmaps: Vec<_> = TrayIcon::ALL.into_iter().map(pixmap).collect();
        for (index, icon) in pixmaps.iter().enumerate() {
            assert_eq!(icon.width, SIZE_PX);
            assert_eq!(icon.data.len(), SIZE * SIZE * 4);
            assert!(
                pixmaps[index + 1..]
                    .iter()
                    .all(|other| other.data != icon.data),
                "{index} collides"
            );
        }
    }

    #[test]
    fn daemon_down_is_a_ring_and_active_is_a_disc() {
        let down = pixmap(TrayIcon::Down);
        let active = pixmap(TrayIcon::Active);
        assert_eq!(center(&down), [0, 0, 0, 0]);
        let (red, green, blue) = TrayIcon::Active.rgb();
        assert_eq!(center(&active), [255, red, green, blue]);
    }

    fn center(icon: &Icon) -> [u8; 4] {
        let start = (SIZE / 2 * SIZE + SIZE / 2) * 4;
        icon.data[start..start + 4].try_into().unwrap()
    }
}

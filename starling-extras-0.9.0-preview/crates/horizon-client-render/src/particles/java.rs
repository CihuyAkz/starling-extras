use super::*;

impl ParticleEngine {
    pub fn has_mod(&self, id: &str) -> bool {
        self.mod_sets.contains_key(id)
    }

    pub fn add_mod(&mut self, id: &str, position: [f64; 3], velocity: [f64; 3], override_limiter: bool) {
        let Some(entry) = self.mod_sets.get(id).cloned() else {
            return;
        };
        let Some((sprite, animated)) = self
            .set_sprites
            .get(entry.set as usize)
            .and_then(|sprites| sprites.first().map(|sprite| (*sprite, sprites.len() > 1)))
        else {
            return;
        };
        if position.iter().chain(velocity.iter()).any(|value| !value.is_finite()) {
            return;
        }
        let camera = self.camera;
        let distance_squared = (position[0] - camera[0]).powi(2)
            + (position[1] - camera[1]).powi(2)
            + (position[2] - camera[2]).powi(2);
        if !override_limiter
            && (distance_squared > 1024.0 || self.calculate_particle_level(false) == 2)
        {
            return;
        }
        let random = &mut self.random;
        let mut particle = Particle::blank(ParticleType::Poof, position);
        particle.velocity = velocity;
        particle.size = 0.1 * (random.next_float() * 0.5 + 0.5) * 2.0;
        particle.lifetime = 10 + random.next_int_bound(20).unwrap_or(0);
        particle.friction = 0.96;
        particle.random = LegacyRandom::new(random.next_long());
        particle.group = entry.group;
        particle.sprite = Sprite::Index(sprite);
        if animated {
            particle.set = Some(entry.set);
            particle.behaviour = Behaviour::Animated;
        }
        *self.made.entry("mod").or_default() += 1;
        self.pending.push(PendingParticle::Ordinary(particle));
    }
}

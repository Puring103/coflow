use coflow_cfc::{CfcContainer, CfcValue, CfcValueRef, ModuleId, ResolveError};

const COMMON: &str = r#"
enum Rarity {
  common = 0,
  rare = 10,
  epic = 20,
  legendary,
}

enum DamageType {
  physical,
  fire,
  ice,
}

type Stats {
  hp: int;
  attack: int;
  speed: float = 1.0;
  flags: int = 0;

  check {
    hp > 0;
    0 <= attack <= 999;
    speed >= 0.1;
    flags == 0 || flags & 1 == 1;
  }
}

type ResistanceTable {
  values: {DamageType: float};

  check {
    len(values) > 0;
    contains(values, DamageType.fire);
    unique(keys(values));
    min(values(values)) >= 0.0;
    max(values(values)) <= 1.0;

    all entry in values {
      entry.value >= 0.0;
      entry.value <= 1.0;
    }
  }
}

type Item {
  id: string;
  rarity: Rarity = Rarity.common;
  tags: [string] = [];
  resistances: ResistanceTable? = null;

  check {
    id != "";

    none tag in tags {
      tag == "";
    }

    resistances is null || contains(resistances.values, DamageType.fire);
  }
}

type SchemaMarker {
  category: "combat" = "combat";
  version: 1 = 1;
  enabled: true = true;
}

type ItemReward {
  item: Item;
  count: int = 1;

  check {
    count > 0;
    item.id != "";
  }
}

type CurrencyReward {
  amount: int;

  check {
    amount > 0;
  }
}

type Reward = ItemReward | CurrencyReward;
"#;

const MONSTERS: &str = r#"
use "common.cfc" as common;

type DropTable {
  rewards: [common.Reward];
  weights: [int];
  tags: [string] = [];

  check {
    len(rewards) == len(weights);
    len(rewards) > 0;
    sum(weights) == 100;
    min(weights) >= 0;
    max(weights) <= 100;
    unique(tags);

    any reward in rewards {
      reward is common.CurrencyReward && reward.amount > 0;
    }

    all reward in rewards {
      reward is common.Reward;
    }

    none tag in tags {
      tag == "";
    }
  }
}

type Monster {
  id: string;
  display: string;
  rarity: common.Rarity;
  stats: common.Stats;
  drops: DropTable;
  primary_drop: common.Reward;
  highlighted_drop: common.Reward;
  optional_boss_drop: common.Item?;
  resistances: {common.DamageType: float};

  check {
    id != "";
    display != "";
    stats.hp > 0;
    rarity >= common.Rarity.common;
    primary_drop is common.Reward;
    highlighted_drop is common.Reward;
    optional_boss_drop is null || optional_boss_drop.rarity >= common.Rarity.rare;
    contains(resistances, common.DamageType.fire);

    all entry in resistances {
      entry.value >= 0.0;
      entry.value <= 1.0;
    }
  }
}

shared_stats: common.Stats = {
  hp: 30,
  attack: 5,
  speed: 1.25,
  flags: 1,
};

fire_resists: {common.DamageType: float} = dict{
  common.DamageType.fire: 0.25,
  common.DamageType.ice: 0.0,
};

schema_marker: common.SchemaMarker = {};

potion: common.Item = {
  id: "potion",
  rarity: common.Rarity.rare,
  tags: ["consumable", "healing"],
  resistances: null,
};

coin_reward: common.CurrencyReward = {
  amount: 25,
};

loot: DropTable = {
  rewards: [
    common.ItemReward {
      item: potion,
      count: 1,
    },
    coin_reward,
  ],
  weights: [40, 60],
  tags: ["starter", "forest"],
};

slime: Monster = {
  id: "slime",
  display: "Green Slime",
  rarity: common.Rarity.common,
  stats: shared_stats,
  drops: loot,
  primary_drop: loot.rewards[0],
  highlighted_drop: primary_drop,
  optional_boss_drop: null,
  resistances: fire_resists,
};

goblin: Monster = {
  id: "goblin",
  display: "Cave Goblin",
  rarity: common.Rarity.rare,
  stats: shared_stats,
  drops: loot,
  primary_drop: highlighted_drop,
  highlighted_drop: drops.rewards[1],
  optional_boss_drop: potion,
  resistances: dict{
    common.DamageType.physical: 0.10,
    common.DamageType.fire: 0.20,
  },
};

monsters: [Monster] = [slime, goblin];
first_reward: common.Reward = loot.rewards[0];

cycle_a = {
  name: "a",
  next: cycle_b,
};

cycle_b = {
  name: "b",
  next: cycle_a,
};

check {
  slime.stats.hp == goblin.stats.hp;
  first_reward is common.ItemReward;
  common.Rarity.rare > common.Rarity.common;

  all monster in monsters {
    monster.stats.hp > 0;
    contains(monster.resistances, common.DamageType.fire);
  }
}
"#;

#[test]
fn spec_complex_feature_example_builds_and_checks() {
    let mut container = CfcContainer::new();
    let result = container
        .load_graph(ModuleId::from("monsters"), MONSTERS, |_, import| {
            if import.path == "common.cfc" {
                Ok((ModuleId::from("common"), COMMON.to_string()))
            } else {
                Err(ResolveError::new("unexpected import"))
            }
        })
        .unwrap();

    let check_errors = container.check(&result);
    assert!(check_errors.is_empty(), "{check_errors:#?}");

    let module = result.root().unwrap();
    let slime = module.get("slime").unwrap();
    let goblin = module.get("goblin").unwrap();
    let shared_stats = module.get("shared_stats").unwrap();
    let slime_stats = object_field(&slime, "stats");
    let goblin_stats = object_field(&goblin, "stats");
    assert!(CfcValueRef::ptr_eq(&slime_stats, &shared_stats));
    assert!(CfcValueRef::ptr_eq(&goblin_stats, &shared_stats));

    let cycle_a = module.get("cycle_a").unwrap();
    let cycle_b = module.get("cycle_b").unwrap();
    let cycle_a_next = object_field(&cycle_a, "next");
    let cycle_b_next = object_field(&cycle_b, "next");
    assert!(CfcValueRef::ptr_eq(&cycle_a_next, &cycle_b));
    assert!(CfcValueRef::ptr_eq(&cycle_b_next, &cycle_a));
}

fn object_field(value: &CfcValueRef, field: &str) -> CfcValueRef {
    let borrowed = value.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };
    fields.get(field).unwrap().clone()
}

//! Attributes
use crate::macros::impl_casts;
use crate::Node;
use serde::Serialize;
use std::any::{Any, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::{Deref, DerefMut, Index, IndexMut};

// TODO: Parameterise attributes over the key type (currently hardcoded to `Node`).

/// Group of attribute stores.
#[derive(Clone)]
pub struct AttrGroup {
    // TODO: Replace RefCell with an AtomicRefCell to allow concurrent
    // borrowing of attributes.
    stores: HashMap<TypeId, RefCell<Box<dyn AttrStoreDyn>>>,
}

impl AttrGroup {
    // PERFORMANCE: We know that the downcasts in each method must always
    // succeed and therefore would not need to perform the check. If the
    // checks turn out to be slow, we can use the unsafe downcast.

    /// Creates an empty [`AttrGroup`].
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
        }
    }

    /// Returns an immutable reference to the store for an attribute.
    ///
    /// # Panics
    ///
    /// - When the attribute is already mutably borrowed.
    /// - When the attribute type is not present in the group.
    #[inline]
    pub fn borrow<T: Attr>(&self) -> AttrRef<T> {
        self.try_borrow().expect("unknown attribute type")
    }

    /// Returns an immutable reference to the store for an attribute,
    /// or `None` when the attribute is not present in the group.
    ///
    /// # Panics
    ///
    /// - When the attribute is already mutably borrowed.
    pub fn try_borrow<T: Attr>(&self) -> Option<AttrRef<T>> {
        self.stores.get(&TypeId::of::<T>()).map(|cell| {
            AttrRef(Ref::map(cell.borrow(), |store| {
                store.downcast_ref().unwrap()
            }))
        })
    }

    /// Returns a mutable reference to the store for an attribute.
    ///
    /// # Panics
    ///
    /// - When the attribute is already mutably borrowed.
    /// - When the attribute type is not present in the group.
    #[inline]
    pub fn borrow_mut<T: Attr>(&self) -> AttrRefMut<T> {
        self.try_borrow_mut().expect("unknown attribute type")
    }

    /// Returns a mutable reference to the store for an attribute.
    ///
    /// # Panics
    ///
    /// - When the attribute is already mutably borrowed.
    pub fn try_borrow_mut<T: Attr>(&self) -> Option<AttrRefMut<T>> {
        self.stores.get(&TypeId::of::<T>()).map(|cell| {
            AttrRefMut(RefMut::map(cell.borrow_mut(), |store| {
                store.downcast_mut().unwrap()
            }))
        })
    }

    /// Returns a mutable reference to the store for an attribute.
    pub fn get_mut<T: Attr>(&mut self) -> Option<&mut T::Store> {
        self.stores
            .get_mut(&TypeId::of::<T>())
            .map(|cell| cell.get_mut().downcast_mut().unwrap())
    }

    /// Removes an attribute store from the group and returns it.
    pub fn take<T: Attr>(&mut self) -> Option<T::Store> {
        self.stores
            .remove(&TypeId::of::<T>())
            .map(|cell| *cell.into_inner().downcast().ok().unwrap())
    }

    /// Inserts an attribute store into the group.
    /// Returns the old store for that attribute type, or `None` if there was none.
    pub fn insert<T: Attr>(&mut self, store: T::Store) -> Option<T::Store> {
        self.stores
            .insert(TypeId::of::<T>(), RefCell::new(Box::new(store)))
            .map(|store| *store.into_inner().downcast().ok().unwrap())
    }

    /// Registers an attribute type in this group.
    /// If the store for the attribute does not already exist,
    /// an empty store for the attribute will be created.
    pub fn register<T: Attr>(&mut self) -> &mut T::Store {
        self.stores
            .entry(TypeId::of::<T>())
            .or_insert_with(|| RefCell::new(Box::<<T as Attr>::Store>::default()))
            .get_mut()
            .downcast_mut()
            .unwrap()
    }

    /// Removes a node from all attribute stores in the group.
    pub fn remove_node(&mut self, node: Node) {
        for store in self.stores.values_mut() {
            store.get_mut().remove(node);
        }
    }
}

impl Default for AttrGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for AttrGroup {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.stores.len()))?;

        for store in self.stores.values() {
            let store_ref = store.borrow();
            map.serialize_entry(store_ref.name(), &store_ref.to_json())?;
        }

        map.end()
    }
}

impl Debug for AttrGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut map = f.debug_map();

        for store in self.stores.values() {
            let store_ref = store.borrow();
            map.entry(&store_ref.name(), &store_ref);
        }

        map.finish()
    }
}

/// Immutable borrow of an attribute store.
///
/// As long as this borrow is alive, the attribute can not be mutably borrowed.
pub struct AttrRef<'a, T>(Ref<'a, T::Store>)
where
    T: Attr;

impl<'a, T> Deref for AttrRef<'a, T>
where
    T: Attr,
{
    type Target = T::Store;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mutable borrow of an attribute store.
///
/// As long as this borrow is alive, it provides exclusive access to the attribute.
/// Any attempt to borrow the attribute again (mutably or immutably) before
/// this reference is dropped will result in a panic.
pub struct AttrRefMut<'a, T>(RefMut<'a, T::Store>)
where
    T: Attr;

impl<'a, T> Deref for AttrRefMut<'a, T>
where
    T: Attr,
{
    type Target = T::Store;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T> DerefMut for AttrRefMut<'a, T>
where
    T: Attr,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Attribute data that can be attached to nodes in a hugr.
pub trait Attr: 'static + Debug + Clone {
    /// Type of the [`AttrStore`] which holds attributes of this type.
    type Store: AttrStore<Attr = Self>;

    /// Name of the attribute.
    ///
    /// This name is implicitly assumed to be unique among all attribute
    /// types that are used together.
    fn name() -> &'static str;
}

/// Internal trait that is used to type erase [`AttrStore`]s
/// so that they can be stored within an [`AttrGroup`].
/// The methods in this trait allow the [`AttrGroup`] to perform
/// operations on the store without knowing the type of the attribute.
trait AttrStoreDyn: Any + Debug + 'static {
    /// Clones the attribute store and returns a trait object for the clone.
    /// This is necessary since the `Clone` trait itself is not object safe.
    fn clone_to_box(&self) -> Box<dyn AttrStoreDyn>;
    /// See [`AttrStore::remove`].
    fn remove(&mut self, node: Node);
    /// See [`AttrStore::to_json`].
    fn to_json(&self) -> serde_json::Value;
    /// See [`AttrStore::name`].
    fn name(&self) -> &'static str;
}

impl Clone for Box<dyn AttrStoreDyn> {
    fn clone(&self) -> Self {
        self.clone_to_box()
    }
}

impl<T> AttrStoreDyn for T
where
    T: AttrStore + 'static,
{
    fn clone_to_box(&self) -> Box<dyn AttrStoreDyn> {
        Box::new(self.clone())
    }

    fn remove(&mut self, node: Node) {
        <T as AttrStore>::remove(self, node);
    }

    fn to_json(&self) -> serde_json::Value {
        <T as AttrStore>::to_json(self)
    }

    fn name(&self) -> &'static str {
        T::Attr::name()
    }
}

impl_casts!(AttrStoreDyn);

/// Storage container for attributes.
pub trait AttrStore: Debug + Clone + Default {
    /// The type of attribute in this store.
    type Attr: Attr<Store = Self>;

    /// Removes the attribute for a node.
    /// Returns the value of the attribute if it existed before.
    fn remove(&mut self, node: Node) -> Option<Self::Attr>;

    /// Inserts an attribute for a node.
    /// Returns the previous value of the attribute if it already existed.
    fn insert(&mut self, node: Node, attr: Self::Attr) -> Option<Self::Attr>;

    /// Returns an immutable reference to the value of an attribute for a node.
    fn get(&self, node: Node) -> Option<&Self::Attr>;

    /// Returns a mutable reference to the value of an attribute for a node.
    fn get_mut(&mut self, node: Node) -> Option<&mut Self::Attr>;

    /// Converts the attribute store to a JSON value.
    fn to_json(&self) -> serde_json::Value;

    // TODO: Iterators
}

/// Attribute store that sparsely stores the attributes in a hashmap.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Sparse<T> {
    data: HashMap<Node, T>,
}

impl<T> Sparse<T>
where
    T: Attr<Store = Self>,
{
    /// Creates an empty [`Sparse`].
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

impl<T> Default for Sparse<T>
where
    T: Attr<Store = Self>,
{
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AttrStore for Sparse<T>
where
    T: Attr<Store = Self> + Serialize,
{
    type Attr = T;

    #[inline]
    fn remove(&mut self, node: Node) -> Option<Self::Attr> {
        self.data.remove(&node)
    }

    #[inline]
    fn insert(&mut self, node: Node, attr: Self::Attr) -> Option<Self::Attr> {
        self.data.insert(node, attr)
    }

    #[inline]
    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    #[inline]
    fn get(&self, node: Node) -> Option<&Self::Attr> {
        self.data.get(&node)
    }

    #[inline]
    fn get_mut(&mut self, node: Node) -> Option<&mut Self::Attr> {
        self.data.get_mut(&node)
    }
}

impl<T> Index<Node> for Sparse<T>
where
    T: Attr<Store = Self>,
{
    type Output = T;

    #[inline]
    fn index(&self, index: Node) -> &Self::Output {
        &self.data[&index]
    }
}

impl<T> IndexMut<Node> for Sparse<T>
where
    T: Attr<Store = Self>,
{
    #[inline]
    fn index_mut(&mut self, index: Node) -> &mut Self::Output {
        self.data.get_mut(&index).unwrap()
    }
}

/// Implement [`Attr`] for an attribute with [`Sparse`] store.
#[macro_export]
macro_rules! impl_attr_sparse {
    ($type:ty, $name:expr) => {
        impl Attr for $type {
            type Store = ::hugr_core::hugr::attributes::Sparse<$type>;

            #[inline]
            fn name() -> &'static str {
                $name
            }
        }
    };
}

pub use impl_attr_sparse;

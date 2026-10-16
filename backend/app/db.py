"""SQLite durable queue and encrypted row repository, schema migration 1."""
import time
import uuid
from contextlib import contextmanager

from sqlalchemy import Float, Integer, String, UniqueConstraint, create_engine, event, select, text
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, sessionmaker

from .config import private_dir
from .crypto import Encrypted


class Base(DeclarativeBase):
    pass


class Record(Base):
    __tablename__ = "records"
    id: Mapped[str] = mapped_column(String, primary_key=True)
    owner: Mapped[str] = mapped_column(String, index=True)
    kind: Mapped[str] = mapped_column(String, index=True)
    version: Mapped[int] = mapped_column(Integer, default=1)
    created: Mapped[float] = mapped_column(Float, default=time.time)
    data = mapped_column(Encrypted, nullable=False)


class Job(Base):
    __tablename__ = "jobs"
    __table_args__ = (UniqueConstraint("owner", "idempotency"),)
    id: Mapped[str] = mapped_column(String, primary_key=True)
    owner: Mapped[str] = mapped_column(String, index=True)
    document_id: Mapped[str] = mapped_column(String, index=True)
    idempotency: Mapped[str] = mapped_column(String)
    digest: Mapped[str] = mapped_column(String)
    state: Mapped[str] = mapped_column(String, default="queued")
    fence: Mapped[int] = mapped_column(Integer, default=0)
    lease: Mapped[float] = mapped_column(Float, default=0)
    created: Mapped[float] = mapped_column(Float, default=time.time)
    data = mapped_column(Encrypted, nullable=False)


class Event(Base):
    __tablename__ = "events"
    seq: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    job_id: Mapped[str] = mapped_column(String, index=True)
    owner: Mapped[str] = mapped_column(String, index=True)
    kind: Mapped[str] = mapped_column(String)
    created: Mapped[float] = mapped_column(Float, default=time.time)


def uid():
    return uuid.uuid4().hex


class Store:
    def __init__(self, settings, keys):
        directory = private_dir(settings.data_dir)
        self.keys = keys
        if any((directory / name).is_symlink() for name in ("icr.sqlite3", "icr.sqlite3-wal", "icr.sqlite3-shm")):
            raise ValueError("Database files cannot be symlinks")
        self.engine = create_engine(f"sqlite:///{directory / 'icr.sqlite3'}",
                                    connect_args={"check_same_thread": False, "timeout": 15})

        @event.listens_for(self.engine, "connect")
        def pragmas(connection, _):
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("PRAGMA foreign_keys=ON")
            connection.execute("PRAGMA busy_timeout=15000")
            connection.execute("PRAGMA secure_delete=ON")

        with self.engine.begin() as connection:
            connection.exec_driver_sql("BEGIN IMMEDIATE")
            version = connection.exec_driver_sql("PRAGMA user_version").scalar()
            if version not in (0, 1):
                raise ValueError("Unsupported database migration")
            Base.metadata.create_all(connection)
            connection.exec_driver_sql("PRAGMA user_version=1")
        (directory / "icr.sqlite3").chmod(0o600)
        self.sessions = sessionmaker(self.engine, expire_on_commit=False)

    @contextmanager
    def transaction(self):
        with self.sessions() as session:
            session.execute(text("BEGIN IMMEDIATE"))
            try:
                yield session
                session.commit()
            except Exception:
                session.rollback()
                raise

    def payload(self, row):
        return self.keys.open(row.data, row.owner, row.id, row.__tablename__ + ":data")

    def save(self, row, payload):
        row.data = self.keys.seal(payload, row.owner, row.id, row.__tablename__ + ":data")

    def record(self, session, owner, kind, payload, row_id=None):
        row = Record(id=row_id or uid(), owner=owner, kind=kind, version=1)
        self.save(row, payload)
        session.add(row)
        return row

    def owned(self, session, row_id, owner, kind=None):
        stmt = select(Record).where(Record.id == row_id, Record.owner == owner)
        if kind:
            stmt = stmt.where(Record.kind == kind)
        return session.scalar(stmt)

    def event(self, session, job, kind):
        session.add(Event(job_id=job.id, owner=job.owner, kind=kind))

    def claim(self, seconds):
        with self.transaction() as session:
            now = time.time()
            expired = session.scalars(select(Job).where(Job.state == "running", Job.lease < now)).all()
            for job in expired:
                job.state, job.fence = "failed", job.fence + 1
                payload = self.payload(job)
                payload["error"] = "worker_lease_expired"
                self.save(job, payload)
                self.event(session, job, "failed")
            if session.scalar(select(Job.id).where(Job.state == "running").limit(1)):
                return None
            job = session.scalar(select(Job).where(Job.state == "queued").order_by(Job.created).limit(1))
            if not job:
                return None
            job.state, job.fence, job.lease = "running", job.fence + 1, now + seconds + 30
            self.event(session, job, "running")
            return job.id, job.fence

    def rotate(self, batch=50):
        count = 0
        with self.transaction() as session:
            for cls in (Record, Job):
                for row in session.scalars(select(cls).order_by(cls.id)).yield_per(batch):
                    import json
                    if json.loads(row.data.value)["kid"] != self.keys.active:
                        self.save(row, self.payload(row))
                        count += 1
                        if count >= batch:
                            return count
        return count

from sqlalchemy import Column, Integer, String
from database import Base
from pydantic import BaseModel
from typing import Optional


class Person(Base):
    """
    Person model for storing user details
    """
    __tablename__ = "person"

    id = Column(Integer, primary_key=True, index=True)
    name = Column(String, index=True)
    email = Column(String, index=True)

    def __repr__(self):
        return f"<Person {self.name}>"

    def __str__(self):
        return {
            "id": self.id,
            "name": self.name,
            "email": self.email,
        }


class CreateOrEditPerson(BaseModel):
    """
    PersonCreate model for creating new person
    """
    id: Optional[int] = None
    name: str
    email: str


class PersonResponse(BaseModel):
    """
    PersonResponse model for returning person details
    """
    id: int
    name: str
    email: str
